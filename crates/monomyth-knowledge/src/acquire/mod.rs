//! Corpus acquisition: fetch, normalize, and ingest ship-safe sources
//! declared in the license ledger.
//!
//! Feature-gated (`acquire`), off by default, so `monomyth-gen` never
//! compiles an HTTP stack (ADR-0010). Ports the retired Python prototype's
//! fetch/normalize flow; **xberg's `Semantic` chunker still owns chunking**
//! (ADR-0007) — this module produces clean `full_text` and hands it to
//! [`crate::Knowledge::ingest`], nothing more.
//!
//! [`build_corpus`] filters the ledger to ship-namespace, non-system,
//! URL-bearing sources before dispatching to a fetcher — this is *defense in
//! depth*, the same three-layer pattern documented on
//! [`crate::build_passage`]: [`Knowledge::ingest`]'s own ledger gate is the
//! authoritative enforcement point, and this up-front filter exists so an
//! obviously-wrong dispatch (e.g. a reference source) is reported as skipped
//! rather than ever reaching the network.

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

/// What happened when [`build_corpus`] considered one ledger source.
#[derive(Debug, Clone)]
pub enum SourceOutcome {
    /// The source was fetched, normalized, and ingested.
    Ingested {
        /// How many works were fetched and ingested.
        count: usize,
    },
    /// The source was filtered out up front (namespace/tier/URL), before any
    /// fetcher dispatch was attempted.
    FilteredOut {
        /// Why the source was filtered out.
        reason: &'static str,
    },
    /// The source is ship-eligible but has no fetcher wired up (or one that
    /// still needs configuration, e.g. a missing archive.org identifier).
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
        let outcome = match classify_entry(entry) {
            EntryDisposition::Filtered(reason) => SourceOutcome::FilteredOut { reason },
            EntryDisposition::Dispatchable => {
                build_one_source(knowledge, &client, &store, entry, opts.limit, retrieved).await
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
    /// Ship-namespace, non-system, URL-bearing — fetchable.
    Dispatchable,
    /// Skipped before any fetch, with a human-readable reason.
    Filtered(&'static str),
}

/// Classify a ledger entry for acquisition. Only ship-namespace, non-system
/// (system sources are authored taxonomy, not fetched text), URL-bearing
/// sources are dispatchable; everything else is [`EntryDisposition::Filtered`]
/// so a `build` run reports *why* a declared source was skipped rather than
/// silently omitting it.
fn classify_entry(entry: &SourceEntry) -> EntryDisposition {
    if entry.tier == Tier::System {
        EntryDisposition::Filtered("system tier: authored taxonomy, not fetched text")
    } else if entry.namespace != Namespace::Ship {
        EntryDisposition::Filtered("reference namespace: informs priors only, never ship-ingested")
    } else if entry.url.is_none() {
        EntryDisposition::Filtered("no source URL declared in the ledger")
    } else {
        EntryDisposition::Dispatchable
    }
}

/// Select the ledger entries `build_corpus` should dispatch to a fetcher:
/// [`EntryDisposition::Dispatchable`] entries, optionally restricted to a single
/// `source_id`. Test-only: `build_corpus` classifies inline (so it can also
/// report [`EntryDisposition::Filtered`] entries); this expresses the
/// dispatchable subset for assertions over the whole ledger.
#[cfg(test)]
fn dispatchable_entries<'ledger>(
    ledger: &'ledger Ledger,
    source_id: Option<&str>,
) -> impl Iterator<Item = &'ledger SourceEntry> {
    ledger.entries().filter(move |entry| {
        source_id.is_none_or(|wanted| entry.id == wanted)
            && matches!(classify_entry(entry), EntryDisposition::Dispatchable)
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
        let ids: Vec<_> = dispatchable_entries(&ledger, None)
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
        let ids: Vec<_> = dispatchable_entries(&ledger, None)
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
    }

    #[test]
    fn dispatchable_entries_excludes_ship_sources_with_no_url() {
        let ledger = ledger();
        let ids: Vec<_> = dispatchable_entries(&ledger, None)
            .map(|entry| entry.id.as_str())
            .collect();
        for entry in dispatchable_entries(&ledger, None) {
            assert!(entry.url.is_some(), "entry '{}' must have a url", entry.id);
        }
        assert!(!ids.is_empty(), "some sources must remain dispatchable");
    }

    #[test]
    fn dispatchable_entries_includes_expected_ship_sources() {
        let ledger = ledger();
        let ids: Vec<_> = dispatchable_entries(&ledger, None)
            .map(|entry| entry.id.as_str())
            .collect();
        for expected in [
            "polti",
            "gutenberg_english",
            "pg_key_works",
            "child_ballads",
            "pg19",
            "bae_reports",
        ] {
            assert!(
                ids.contains(&expected),
                "expected '{expected}' to be dispatchable"
            );
        }
    }

    #[test]
    fn dispatchable_entries_restricts_to_a_single_source_id() {
        let ledger = ledger();
        let ids: Vec<_> = dispatchable_entries(&ledger, Some("polti"))
            .map(|entry| entry.id.as_str())
            .collect();
        assert_eq!(ids, vec!["polti"]);
    }

    #[test]
    fn classify_entry_filters_system_tier_with_a_reason() {
        let ledger = ledger();
        let entry = ledger.get("campbell_monomyth").expect("declared");
        assert!(matches!(
            classify_entry(entry),
            EntryDisposition::Filtered(reason) if reason.contains("system")
        ));
    }

    #[test]
    fn classify_entry_filters_reference_namespace_with_a_reason() {
        let ledger = ledger();
        let entry = ledger.get("perseus").expect("declared");
        assert!(matches!(
            classify_entry(entry),
            EntryDisposition::Filtered(reason) if reason.contains("reference")
        ));
    }

    #[test]
    fn classify_entry_dispatches_a_ship_url_source() {
        let ledger = ledger();
        let entry = ledger.get("child_ballads").expect("declared");
        assert!(matches!(
            classify_entry(entry),
            EntryDisposition::Dispatchable
        ));
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
