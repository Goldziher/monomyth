//! Corpus acquisition: fetch, normalize, and ingest ship-safe sources
//! declared in the license ledger; separately, download reference/unverified
//! sources into an inspect-only area for human license review.
//!
//! Feature-gated (`acquire`), off by default, so `monomyth-gen` never
//! compiles an HTTP stack (ADR-0010). Ports the retired Python prototype's
//! fetch/normalize flow; **xberg's `Semantic` chunker still owns chunking**
//! (ADR-0007) — this module produces clean `full_text` and hands it to
//! [`crate::Knowledge::ingest`], nothing more.
//!
//! Two acquisition modes ([`AcquireMode`]) exist because ship ingestion and
//! reference download-for-inspection are never allowed to be conflated
//! (ADR-0005): [`build_corpus`] filters the ledger to ship-namespace,
//! non-system, URL-bearing sources before dispatching to a fetcher — this is
//! *defense in depth*, the same three-layer pattern documented on
//! [`crate::build_passage`]: [`Knowledge::ingest`]'s own ledger gate is the
//! authoritative enforcement point, and this up-front filter exists so an
//! obviously-wrong dispatch (e.g. a reference source) is reported as skipped
//! rather than ever reaching the network. [`inspect_corpus`] mirrors that
//! filter in the opposite direction — reference-namespace, non-system,
//! URL-bearing sources only — and structurally cannot ingest anything: its
//! per-source worker, [`inspect_one_source`], never calls
//! [`Knowledge::ingest`].

pub mod error;
mod fetch;
mod normalize;

pub use error::AcquireError;
pub use fetch::FetchedWork;
mod http;
mod storage;

#[cfg(test)]
use crate::ledger::Ledger;
use crate::ledger::{Namespace, SourceEntry, Tier};
use crate::{Knowledge, KnowledgeError};

use fetch::gutendex::{self, SearchBy};
use fetch::huggingface::{self, DatasetSpec};
use http::CacheMode;

/// Options controlling a [`build_corpus`] run.
#[derive(Debug, Clone, Default)]
pub struct BuildOptions {
    /// Restrict acquisition to a single declared source id. `None` builds
    /// every dispatchable source in the ledger.
    pub source: Option<String>,
    /// Cap the number of works fetched and ingested per source. `None` uses
    /// each fetcher family's own default (a single work for Gutendex/archive
    /// sources, or all paginated rows for `HuggingFace` sources).
    pub limit: Option<usize>,
}

/// Default number of rows fetched from a `HuggingFace` dataset source when no
/// `limit` is given, so an unbounded `build` cannot page a dataset forever.
const DEFAULT_HUGGINGFACE_LIMIT: usize = 20;

/// Ledger source ids whose text arrives as raw Project Gutenberg pages and so
/// needs [`normalize::strip_gutenberg`] before paragraph cleanup.
///
/// Single source of truth shared by [`fetch_for_source`]'s dispatch and
/// [`normalize_for_family`]: adding a Gutendex-backed source to one without the
/// other would silently ship raw PG boilerplate, itself a licensing regression
/// (stripping the trademark header is a license term — see [`normalize`]).
const GUTENDEX_FAMILY_SOURCE_IDS: [&str; 3] = ["polti", "pg_key_works", "child_ballads"];

/// What happened when [`build_corpus`] or [`inspect_corpus`] considered one
/// ledger source.
#[derive(Debug, Clone)]
pub enum SourceOutcome {
    /// The source was fetched, normalized, and ingested.
    Ingested {
        /// How many works were fetched and ingested.
        count: usize,
    },
    /// The reference blob was fetched into the `reference/` prefix for
    /// inspection, not ingested. Only produced by [`inspect_corpus`] — the
    /// structural counterpart to `Ingested` that keeps the two acquisition
    /// modes' reports visibly distinct even when printed side by side.
    Downloaded {
        /// How many works were fetched into the inspect area.
        count: usize,
    },
    /// The source was filtered out up front (namespace/tier/URL), before any
    /// fetcher dispatch was attempted.
    FilteredOut {
        /// Why the source was filtered out.
        reason: &'static str,
    },
    /// The source is eligible for the current mode but has no fetcher wired
    /// up (or one that still needs configuration, e.g. a missing
    /// archive.org identifier).
    NoFetcher {
        /// Why no fetcher is available.
        reason: &'static str,
    },
    /// A fetcher was dispatched but failed.
    Failed {
        /// The error the fetcher or ingest call returned.
        error: String,
    },
}

/// The per-source outcome of one [`build_corpus`] run.
#[derive(Debug, Clone)]
pub struct SourceReport {
    /// The ledger source id.
    pub source_id: String,
    /// What happened for this source.
    pub outcome: SourceOutcome,
}

/// The full report of a [`build_corpus`] run.
#[derive(Debug, Clone, Default)]
pub struct BuildReport {
    /// One entry per ledger source considered.
    pub sources: Vec<SourceReport>,
}

impl BuildReport {
    /// Total works ingested across all sources.
    #[must_use]
    pub fn total_ingested(&self) -> usize {
        self.sources
            .iter()
            .map(|report| match &report.outcome {
                SourceOutcome::Ingested { count } => *count,
                _ => 0,
            })
            .sum()
    }

    /// Total works downloaded for inspection (never ingested) across all
    /// sources. Mirrors [`Self::total_ingested`] for [`inspect_corpus`]
    /// runs.
    #[must_use]
    pub fn total_downloaded(&self) -> usize {
        self.sources
            .iter()
            .map(|report| match &report.outcome {
                SourceOutcome::Downloaded { count } => *count,
                _ => 0,
            })
            .sum()
    }
}

/// Which acquisition mode a run performs. The two modes are kept as a typed
/// enum, rather than a bool or two near-duplicate functions, so a caller
/// cannot accidentally conflate ship ingestion with reference download — the
/// distinction that this whole task exists to enforce structurally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireMode {
    /// Fetch, normalize, and ingest ship-namespace sources into the
    /// surfaceable ship collection.
    BuildShip,
    /// Fetch reference/unverified sources into the `reference/` blob prefix
    /// for human license/provenance review, without ever calling
    /// [`Knowledge::ingest`].
    InspectReference,
}

/// Fetch, normalize, and ingest every ship-safe, dispatchable source declared
/// in `knowledge`'s ledger (or just `opts.source`, if given).
///
/// `retrieved` is the retrieval date (`YYYY-MM-DD`) stamped onto every
/// ingested work's provenance; callers pass it in rather than this function
/// calling a live clock, so the pipeline stays a pure function of its inputs
/// (the one live clock call belongs at the CLI call site). It is validated at
/// this boundary — a malformed date must not silently reach stored provenance.
///
/// # Errors
///
/// Returns [`KnowledgeError::Acquire`] if `retrieved` is not a valid
/// `YYYY-MM-DD` date or if the shared HTTP client cannot be constructed.
/// Per-source fetch/normalize/ingest failures are captured in the returned
/// [`BuildReport`] as [`SourceOutcome::Failed`] rather than aborting the run.
pub async fn build_corpus(
    knowledge: &Knowledge,
    opts: BuildOptions,
    retrieved: &str,
) -> Result<BuildReport, KnowledgeError> {
    acquire_corpus(knowledge, opts, retrieved, AcquireMode::BuildShip).await
}

/// Fetch reference/unverified sources declared in `knowledge`'s ledger (or
/// just `opts.source`, if given) into the `reference/` blob prefix
/// (ADR-0012), for human license and provenance review.
///
/// This function **never** calls [`Knowledge::ingest`] — reference material
/// must never enter the ship collection (ADR-0005). `knowledge` is still
/// taken by reference because the entry loop reads
/// [`Knowledge::ledger`]; it is ledger access, not ingestion, and no code
/// path from here reaches `.ingest(`.
///
/// # Errors
///
/// As [`build_corpus`]: [`KnowledgeError::Acquire`] if `retrieved` is
/// malformed or the shared HTTP client cannot be constructed. Per-source
/// failures are captured in the returned [`BuildReport`] rather than
/// aborting the run.
pub async fn inspect_corpus(
    knowledge: &Knowledge,
    opts: BuildOptions,
    retrieved: &str,
) -> Result<BuildReport, KnowledgeError> {
    acquire_corpus(knowledge, opts, retrieved, AcquireMode::InspectReference).await
}

/// The shared acquisition loop behind [`build_corpus`] and
/// [`inspect_corpus`]: classify every ledger entry under `mode`, then
/// dispatch each to ingestion, inspect-download, or a filtered-out report.
async fn acquire_corpus(
    knowledge: &Knowledge,
    opts: BuildOptions,
    retrieved: &str,
    mode: AcquireMode,
) -> Result<BuildReport, KnowledgeError> {
    validate_retrieved_date(retrieved)?;

    let client = http::build_client().map_err(|source| {
        KnowledgeError::from(AcquireError::Http {
            url: "(client construction)".to_owned(),
            attempts: 0,
            source,
        })
    })?;
    let store = storage::BlobStore::local().map_err(KnowledgeError::from)?;

    let mut report = BuildReport::default();
    for entry in knowledge.ledger().entries() {
        if opts
            .source
            .as_deref()
            .is_some_and(|wanted| entry.id != wanted)
        {
            continue;
        }
        let outcome = match classify_entry(entry, mode) {
            EntryDisposition::Filtered(reason) => SourceOutcome::FilteredOut { reason },
            EntryDisposition::Dispatchable => {
                build_one_source(knowledge, &client, &store, entry, opts.limit, retrieved).await
            }
            EntryDisposition::Inspectable => {
                inspect_one_source(&client, &store, entry, opts.limit, retrieved).await
            }
        };
        report.sources.push(SourceReport {
            source_id: entry.id.clone(),
            outcome,
        });
    }
    Ok(report)
}

/// The `YYYY-MM-DD` format every stored `retrieved` date is validated against.
const RETRIEVED_DATE_FORMAT: &[time::format_description::FormatItem<'_>] =
    time::macros::format_description!("[year]-[month]-[day]");

/// Validate that `retrieved` parses as a `YYYY-MM-DD` date, so a malformed
/// caller-supplied date is rejected at the pipeline boundary rather than
/// silently written into stored provenance (ADR-0005).
fn validate_retrieved_date(retrieved: &str) -> Result<(), AcquireError> {
    time::Date::parse(retrieved, RETRIEVED_DATE_FORMAT)
        .map(|_date| ())
        .map_err(|_error| AcquireError::InvalidInput {
            what: "retrieved date (expected YYYY-MM-DD)",
            value: retrieved.to_owned(),
        })
}

/// Whether a ledger entry can be dispatched to a fetcher, or why it was skipped.
enum EntryDisposition {
    /// Ship-namespace, non-system, URL-bearing, under [`AcquireMode::BuildShip`]
    /// — fetchable and ingestible.
    Dispatchable,
    /// Reference-namespace, non-system, URL-bearing, under
    /// [`AcquireMode::InspectReference`] — fetchable into the inspect area,
    /// but never ingested.
    Inspectable,
    /// Skipped before any fetch, with a human-readable reason.
    Filtered(&'static str),
}

/// Classify a ledger entry for acquisition under `mode`. System-tier sources
/// (authored taxonomy, not fetched text) and URL-less sources are always
/// filtered. Otherwise the namespace must match the mode: ship-namespace
/// sources are [`EntryDisposition::Dispatchable`] only under
/// [`AcquireMode::BuildShip`]; reference-namespace sources are
/// [`EntryDisposition::Inspectable`] only under
/// [`AcquireMode::InspectReference`]. A namespace/mode mismatch is filtered
/// with a reason, so a run reports *why* a declared source was skipped
/// rather than silently omitting it — this is the enforcement point that
/// keeps `corpus build` from ever touching a reference source and `corpus
/// inspect` from ever ingesting a ship source.
fn classify_entry(entry: &SourceEntry, mode: AcquireMode) -> EntryDisposition {
    if entry.tier == Tier::System {
        return EntryDisposition::Filtered("system tier: authored taxonomy, not fetched text");
    }
    if entry.url.is_none() {
        return EntryDisposition::Filtered("no source URL declared in the ledger");
    }
    match (mode, entry.namespace) {
        (AcquireMode::BuildShip, Namespace::Ship) => EntryDisposition::Dispatchable,
        (AcquireMode::BuildShip, Namespace::Reference) => EntryDisposition::Filtered(
            "reference namespace: informs priors only, never ship-ingested",
        ),
        (AcquireMode::InspectReference, Namespace::Reference) => EntryDisposition::Inspectable,
        (AcquireMode::InspectReference, Namespace::Ship) => {
            EntryDisposition::Filtered("ship namespace: use `corpus build`, not inspect")
        }
    }
}

/// Select the ledger entries `build_corpus` should dispatch to a fetcher:
/// [`EntryDisposition::Dispatchable`] entries under `mode`, optionally
/// restricted to a single `source_id`. Test-only: `acquire_corpus` classifies
/// inline (so it can also report [`EntryDisposition::Filtered`] and
/// [`EntryDisposition::Inspectable`] entries); this expresses the
/// dispatchable subset for assertions over the whole ledger.
#[cfg(test)]
fn dispatchable_entries<'ledger>(
    ledger: &'ledger Ledger,
    source_id: Option<&str>,
    mode: AcquireMode,
) -> impl Iterator<Item = &'ledger SourceEntry> {
    ledger.entries().filter(move |entry| {
        source_id.is_none_or(|wanted| entry.id == wanted)
            && matches!(classify_entry(entry, mode), EntryDisposition::Dispatchable)
    })
}

/// Fetch, normalize, and ingest one ledger source, mapping every failure mode
/// to a [`SourceOutcome`] rather than a `Result` — a single source's failure
/// must not abort the rest of the build.
async fn build_one_source(
    knowledge: &Knowledge,
    client: &reqwest::Client,
    store: &storage::BlobStore,
    entry: &SourceEntry,
    limit: Option<usize>,
    retrieved: &str,
) -> SourceOutcome {
    let works = match fetch_for_source(client, store, entry, limit, retrieved).await {
        Ok(works) => works,
        Err(DispatchOutcome::NoFetcher { reason }) => return SourceOutcome::NoFetcher { reason },
        Err(DispatchOutcome::Error(error)) => {
            return SourceOutcome::Failed {
                error: error.to_string(),
            };
        }
    };

    let mut ingested = 0;
    for work in works {
        let normalized = normalize_for_family(entry, &work);
        let input = crate::IngestInput {
            title: work.title.clone(),
            source_uri: Some(work.url.clone()),
            checksum: Some(work.checksum.clone()),
            retrieved: Some(work.retrieved.clone()),
            ..crate::IngestInput::new(normalized)
        };
        match knowledge.ingest(&entry.id, input).await {
            Ok(_document_id) => ingested += 1,
            Err(error) => {
                return SourceOutcome::Failed {
                    error: error.to_string(),
                };
            }
        }
    }
    SourceOutcome::Ingested { count: ingested }
}

/// Fetch one reference/unverified ledger source into the inspect area,
/// mapping every failure mode to a [`SourceOutcome`] exactly as
/// [`build_one_source`] does. The one structural difference — and the entire
/// point of this function existing separately — is that it takes no
/// `knowledge` parameter and never calls [`Knowledge::ingest`] anywhere in
/// its body: there is no code path from here into the ship collection.
async fn inspect_one_source(
    client: &reqwest::Client,
    store: &storage::BlobStore,
    entry: &SourceEntry,
    limit: Option<usize>,
    retrieved: &str,
) -> SourceOutcome {
    match fetch_for_source(client, store, entry, limit, retrieved).await {
        Ok(works) => SourceOutcome::Downloaded { count: works.len() },
        Err(DispatchOutcome::NoFetcher { reason }) => SourceOutcome::NoFetcher { reason },
        Err(DispatchOutcome::Error(error)) => SourceOutcome::Failed {
            error: error.to_string(),
        },
    }
}

/// Why fetch dispatch for a source did not produce works.
enum DispatchOutcome {
    /// No fetcher is wired up (or needs configuration) for this source.
    NoFetcher { reason: &'static str },
    /// A fetcher was dispatched but returned an error.
    Error(AcquireError),
}

impl From<AcquireError> for DispatchOutcome {
    fn from(error: AcquireError) -> Self {
        Self::Error(error)
    }
}

/// Dispatch one ledger source to its fetcher family, by source id.
///
/// See the module doc comment for the dispatch table this mirrors; sources
/// with no generic fetcher (SPARQL endpoints, DOI/zenodo archives, a bare
/// GitHub repo, a plain HTTP site with no structured API) report
/// [`DispatchOutcome::NoFetcher`].
async fn fetch_for_source(
    client: &reqwest::Client,
    store: &storage::BlobStore,
    entry: &SourceEntry,
    limit: Option<usize>,
    retrieved: &str,
) -> Result<Vec<FetchedWork>, DispatchOutcome> {
    let ctx = http::FetchContext::new(
        client,
        store,
        storage::StoragePrefix::from(entry.namespace),
        CacheMode::Enabled,
    );
    match entry.id.as_str() {
        "polti" => {
            let work = fetch_first_gutendex_search(
                &ctx,
                SearchBy::Query("polti dramatic situations".to_owned()),
                retrieved,
            )
            .await?;
            Ok(vec![work])
        }
        "pg_key_works" => {
            let work = fetch_first_gutendex_search(
                &ctx,
                SearchBy::Topic("mythology".to_owned()),
                retrieved,
            )
            .await?;
            Ok(vec![work])
        }
        "child_ballads" => {
            // `classify_entry`'s `entry.url.is_none()` guard filters URL-less
            // sources before dispatch, so a dispatched source always has a URL;
            // `unwrap_or_default` yields `""` only in the unreachable case,
            // which `parse_ebook_id` then rejects as a parse error.
            let url = entry.url.as_deref().unwrap_or_default();
            let pg_id = gutendex::parse_ebook_id(url)?;
            let work = gutendex::fetch(&ctx, pg_id, None, None, retrieved).await?;
            Ok(vec![work])
        }
        "gutenberg_english" => {
            fetch_huggingface(
                &ctx,
                DatasetSpec {
                    dataset: "sedthh/gutenberg_english".to_owned(),
                    config: "default".to_owned(),
                    split: "train".to_owned(),
                    text_column: "TEXT".to_owned(),
                },
                limit,
                retrieved,
            )
            .await
        }
        "pg19" => {
            fetch_huggingface(
                &ctx,
                DatasetSpec {
                    dataset: "deepmind/pg19".to_owned(),
                    config: "default".to_owned(),
                    split: "train".to_owned(),
                    text_column: "text".to_owned(),
                },
                limit,
                retrieved,
            )
            .await
        }
        "bae_reports" => Err(DispatchOutcome::NoFetcher {
            reason: "needs a real archive.org identifier wired in (none declared in the manifest)",
        }),
        "trilogy" => Err(DispatchOutcome::NoFetcher {
            reason: "github repository URL; no generic git fetcher",
        }),
        "bag_of_tales" => Err(DispatchOutcome::NoFetcher {
            reason: "DOI/zenodo URL; no generic DOI resolver",
        }),
        "wikidata_myth" | "dbpedia_myth" => Err(DispatchOutcome::NoFetcher {
            reason: "SPARQL endpoint; no generic SPARQL fetcher",
        }),
        "iapsop" => Err(DispatchOutcome::NoFetcher {
            reason: "plain HTTP site with no structured API; no generic HTML fetcher",
        }),
        _ => Err(DispatchOutcome::NoFetcher {
            reason: "no fetcher wired up for this source id",
        }),
    }
}

/// Search Gutendex and fetch the most popular PD candidate's plain text.
///
/// Gutendex orders results by `download_count` descending already, but this
/// picks explicitly by that field (rather than list order) so popularity —
/// our quality proxy for an unattended "key work" pick — stays load-bearing
/// even if Gutendex's own ordering ever changes.
async fn fetch_first_gutendex_search(
    ctx: &http::FetchContext<'_>,
    by: SearchBy,
    retrieved: &str,
) -> Result<FetchedWork, DispatchOutcome> {
    let candidates = gutendex::search(ctx, &by).await?;
    let candidate = candidates
        .into_iter()
        .filter(|candidate| candidate.text_url.is_some())
        .max_by_key(|candidate| candidate.download_count)
        .ok_or(DispatchOutcome::NoFetcher {
            reason: "gutendex search returned no candidate with a plain-text format",
        })?;
    let title = titled_with_authors(&candidate.title, &candidate.authors);
    let work = gutendex::fetch(
        ctx,
        candidate.pg_id,
        candidate.text_url.as_deref(),
        Some(title),
        retrieved,
    )
    .await?;
    Ok(work)
}

/// Build a title string carrying attribution: `"<title> — <author1>, <author2>"`
/// when Gutendex reports author names, else the bare title.
fn titled_with_authors(title: &str, authors: &[String]) -> String {
    if authors.is_empty() {
        title.to_owned()
    } else {
        format!("{title} — {}", authors.join(", "))
    }
}

/// Fetch rows from a `HuggingFace` dataset source, applying the caller's
/// `limit` or [`DEFAULT_HUGGINGFACE_LIMIT`].
async fn fetch_huggingface(
    ctx: &http::FetchContext<'_>,
    spec: DatasetSpec,
    limit: Option<usize>,
    retrieved: &str,
) -> Result<Vec<FetchedWork>, DispatchOutcome> {
    let effective_limit = limit.unwrap_or(DEFAULT_HUGGINGFACE_LIMIT);
    let works = huggingface::fetch_rows(
        ctx,
        &spec,
        effective_limit,
        huggingface::clamp_page_length(100),
        retrieved,
    )
    .await?;
    Ok(works)
}

/// Normalize a fetched work's text for its fetcher family: Gutendex-family
/// works are raw Project Gutenberg pages and need [`normalize::strip_gutenberg`]
/// before paragraph cleanup; `HuggingFace`/archive.org rows are already
/// pre-cleaned dataset text with no PG boilerplate to strip, so only
/// [`normalize::clean_full_text`] runs.
fn normalize_for_family(entry: &SourceEntry, work: &FetchedWork) -> String {
    let is_gutendex_family = GUTENDEX_FAMILY_SOURCE_IDS.contains(&entry.id.as_str());
    if is_gutendex_family {
        let (stripped, _was_stripped) = normalize::strip_gutenberg(&work.full_text);
        normalize::clean_full_text(&stripped)
    } else {
        normalize::clean_full_text(&work.full_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::Ledger;

    fn ledger() -> Ledger {
        Ledger::load_embedded().expect("embedded manifest parses")
    }

    #[test]
    fn validate_retrieved_date_accepts_a_well_formed_date() {
        assert!(validate_retrieved_date("2026-07-11").is_ok());
    }

    #[test]
    fn validate_retrieved_date_rejects_a_malformed_date() {
        let error = validate_retrieved_date("11/07/2026").expect_err("must be rejected");
        match error {
            AcquireError::InvalidInput { what, value } => {
                assert_eq!(what, "retrieved date (expected YYYY-MM-DD)");
                assert_eq!(value, "11/07/2026");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn titled_with_authors_appends_joined_author_names() {
        assert_eq!(
            titled_with_authors("Myths and Legends", &["A. Storyteller".to_owned()]),
            "Myths and Legends — A. Storyteller"
        );
        assert_eq!(
            titled_with_authors(
                "Old Tales",
                &["A. Storyteller".to_owned(), "B. Scribe".to_owned()]
            ),
            "Old Tales — A. Storyteller, B. Scribe"
        );
    }

    #[test]
    fn titled_with_authors_returns_bare_title_with_no_authors() {
        assert_eq!(titled_with_authors("Untitled Work", &[]), "Untitled Work");
    }

    #[test]
    fn dispatchable_entries_excludes_system_tier_sources() {
        let ledger = ledger();
        let ids: Vec<_> = dispatchable_entries(&ledger, None, AcquireMode::BuildShip)
            .map(|entry| entry.id.as_str())
            .collect();
        assert!(
            !ids.contains(&"campbell_monomyth"),
            "system tier must be excluded"
        );
        assert!(!ids.contains(&"propp"), "system tier must be excluded");
    }

    #[test]
    fn dispatchable_entries_excludes_reference_namespace_sources() {
        let ledger = ledger();
        let ids: Vec<_> = dispatchable_entries(&ledger, None, AcquireMode::BuildShip)
            .map(|entry| entry.id.as_str())
            .collect();
        assert!(
            !ids.contains(&"perseus"),
            "reference namespace must be excluded"
        );
        assert!(
            !ids.contains(&"duchas"),
            "reference namespace must be excluded"
        );
        assert!(
            !ids.contains(&"tvtropes"),
            "reference namespace must be excluded"
        );
        assert!(
            !ids.contains(&"gutenberg_english"),
            "gutenberg_english is now reference namespace and must be excluded from ship dispatch"
        );
        assert!(
            !ids.contains(&"pg19"),
            "pg19 is now reference namespace and must be excluded from ship dispatch"
        );
    }

    #[test]
    fn dispatchable_entries_excludes_ship_sources_with_no_url() {
        let ledger = ledger();
        let ids: Vec<_> = dispatchable_entries(&ledger, None, AcquireMode::BuildShip)
            .map(|entry| entry.id.as_str())
            .collect();
        for entry in dispatchable_entries(&ledger, None, AcquireMode::BuildShip) {
            assert!(entry.url.is_some(), "entry '{}' must have a url", entry.id);
        }
        assert!(!ids.is_empty(), "some sources must remain dispatchable");
    }

    #[test]
    fn dispatchable_entries_includes_expected_ship_sources() {
        let ledger = ledger();
        let ids: Vec<_> = dispatchable_entries(&ledger, None, AcquireMode::BuildShip)
            .map(|entry| entry.id.as_str())
            .collect();
        for expected in ["polti", "pg_key_works", "child_ballads", "bae_reports"] {
            assert!(
                ids.contains(&expected),
                "expected '{expected}' to be dispatchable"
            );
        }
    }

    #[test]
    fn dispatchable_entries_restricts_to_a_single_source_id() {
        let ledger = ledger();
        let ids: Vec<_> = dispatchable_entries(&ledger, Some("polti"), AcquireMode::BuildShip)
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(ids, vec!["polti"]);
    }

    #[test]
    fn classify_entry_filters_system_tier_with_a_reason_under_both_modes() {
        let ledger = ledger();
        let entry = ledger.get("campbell_monomyth").expect("declared");
        for mode in [AcquireMode::BuildShip, AcquireMode::InspectReference] {
            assert!(matches!(
                classify_entry(entry, mode),
                EntryDisposition::Filtered(reason) if reason.contains("system")
            ));
        }
    }

    #[test]
    fn classify_entry_filters_a_url_less_source_under_both_modes() {
        let ledger = ledger();
        let entry = ledger.get("fandom_dumps").expect("declared");
        assert!(
            entry.url.is_none(),
            "fandom_dumps must declare no url for this test to be meaningful"
        );
        for mode in [AcquireMode::BuildShip, AcquireMode::InspectReference] {
            assert!(matches!(
                classify_entry(entry, mode),
                EntryDisposition::Filtered(reason) if reason.contains("no source URL")
            ));
        }
    }

    #[test]
    fn classify_entry_filters_reference_namespace_with_a_reason_under_build_ship() {
        let ledger = ledger();
        let entry = ledger.get("perseus").expect("declared");
        assert!(matches!(
            classify_entry(entry, AcquireMode::BuildShip),
            EntryDisposition::Filtered(reason) if reason.contains("reference")
        ));
    }

    #[test]
    fn classify_entry_filters_ship_namespace_with_a_reason_under_inspect_reference() {
        let ledger = ledger();
        let entry = ledger.get("child_ballads").expect("declared");
        assert!(matches!(
            classify_entry(entry, AcquireMode::InspectReference),
            EntryDisposition::Filtered(reason) if reason.contains("corpus build")
        ));
    }

    #[test]
    fn classify_entry_dispatches_a_ship_url_source_under_build_ship() {
        let ledger = ledger();
        let entry = ledger.get("child_ballads").expect("declared");
        assert!(matches!(
            classify_entry(entry, AcquireMode::BuildShip),
            EntryDisposition::Dispatchable
        ));
    }

    #[test]
    fn classify_entry_marks_a_reference_url_source_inspectable_under_inspect_reference() {
        let ledger = ledger();
        let entry = ledger.get("perseus").expect("declared");
        assert!(matches!(
            classify_entry(entry, AcquireMode::InspectReference),
            EntryDisposition::Inspectable
        ));
    }

    /// The licensing point of this whole task: the two bulk `HuggingFace`
    /// datasets whose per-item PD status is unverified must classify as
    /// `Inspectable` (never `Dispatchable`) under `InspectReference`, and be
    /// `Filtered` (never dispatched) under `BuildShip`.
    #[test]
    fn classify_entry_routes_gutenberg_english_and_pg19_to_inspect_only() {
        let ledger = ledger();
        for id in ["gutenberg_english", "pg19"] {
            let entry = ledger.get(id).expect("declared");
            assert!(
                matches!(
                    classify_entry(entry, AcquireMode::BuildShip),
                    EntryDisposition::Filtered(_)
                ),
                "'{id}' must be filtered out under BuildShip"
            );
            assert!(
                matches!(
                    classify_entry(entry, AcquireMode::InspectReference),
                    EntryDisposition::Inspectable
                ),
                "'{id}' must be inspectable under InspectReference"
            );
        }
    }

    /// Ledger-level trace-through: a source declared with `namespace:
    /// "reference"` in the manifest resolves to `Namespace::Reference` and
    /// the `reference` tier for both re-namespaced sources.
    #[test]
    fn gutenberg_english_and_pg19_are_reference_namespace_and_reference_tier() {
        let ledger = ledger();
        for id in ["gutenberg_english", "pg19"] {
            let entry = ledger.get(id).expect("declared");
            assert_eq!(
                entry.namespace,
                Namespace::Reference,
                "'{id}' must be reference-namespace"
            );
            assert_eq!(entry.tier, Tier::Reference, "'{id}' must be reference-tier");
        }
    }

    /// Ledger-level trace-through of the ADR-0012 storage prefix: a
    /// reference-namespace ledger entry resolves to the reference storage
    /// prefix, never the ship prefix.
    #[test]
    fn reference_namespace_ledger_entry_resolves_to_the_reference_storage_prefix() {
        let ledger = ledger();
        let entry = ledger.get("gutenberg_english").expect("declared");
        assert_eq!(
            storage::StoragePrefix::from(entry.namespace),
            storage::StoragePrefix::Reference
        );
        assert_eq!(
            storage::StoragePrefix::from(Namespace::Reference),
            storage::StoragePrefix::Reference
        );
    }

    #[test]
    fn normalize_for_family_strips_gutenberg_boilerplate_for_gutendex_sources() {
        let ledger = ledger();
        let entry = ledger.get("polti").expect("polti is declared");
        let work = FetchedWork {
            full_text: "*** START OF THE PROJECT GUTENBERG EBOOK X ***\n\nBody text here.\n\n*** END OF THE PROJECT GUTENBERG EBOOK X ***".to_owned(),
            url: "https://example.org".to_owned(),
            checksum: "sha256:abc".to_owned(),
            retrieved: "2026-07-11".to_owned(),
            title: None,
        };
        assert_eq!(normalize_for_family(entry, &work), "Body text here.");
    }

    #[test]
    fn normalize_for_family_skips_gutenberg_stripping_for_huggingface_sources() {
        let ledger = ledger();
        let entry = ledger.get("gutenberg_english").expect("declared");
        let work = FetchedWork {
            full_text: "Already-clean   dataset\nrow text.".to_owned(),
            url: "https://example.org".to_owned(),
            checksum: "sha256:abc".to_owned(),
            retrieved: "2026-07-11".to_owned(),
            title: None,
        };
        assert_eq!(
            normalize_for_family(entry, &work),
            "Already-clean dataset row text."
        );
    }
}
