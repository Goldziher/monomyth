//! Dispatch table: which fetcher family (if any) a ledger source id maps to,
//! and the per-family glue that turns a [`SourceEntry`] into fetched works.
//!
//! Split out of [`crate::acquire`] (which owns the acquisition loop and
//! ship/reference classification) once the number of fetcher families grew
//! past the three the module started with — this file is the one place that
//! knows *which* family a source id belongs to, so adding a family never
//! touches the orchestration logic in `mod.rs`.
//!
//! [`route`] is the pure id → fetcher-family mapping, factored out of the
//! async [`fetch_for_source`] specifically so dispatch tests can assert a
//! source id resolves to a real fetcher without ever touching the network —
//! see the `route_resolves_*` tests below.

use crate::ledger::SourceEntry;

use super::error::AcquireError;
use super::fetch::git::RepoDirSpec;
use super::fetch::gutendex::{self, SearchBy};
use super::fetch::huggingface::{self, DatasetSpec};
use super::fetch::{FetchedWork, archive, git, sparql, zenodo};
use super::http::{self, CacheMode, FetchContext};
use super::storage;

/// Ledger source ids whose text arrives as raw Project Gutenberg pages and so
/// needs [`super::normalize::strip_gutenberg`] before paragraph cleanup.
///
/// Single source of truth shared by [`route`]'s dispatch and
/// [`normalize_for_family`]: adding a Gutendex-backed source to one without the
/// other would silently ship raw PG boilerplate, itself a licensing regression
/// (stripping the trademark header is a license term — see
/// [`super::normalize`]).
const GUTENDEX_FAMILY_SOURCE_IDS: [&str; 3] = ["polti", "pg_key_works", "child_ballads"];

/// Default number of rows fetched from a `HuggingFace` dataset source when no
/// `limit` is given, so an unbounded `build` cannot page a dataset forever.
const DEFAULT_HUGGINGFACE_LIMIT: usize = 20;

/// The archive.org identifier fetched for `bae_reports`: a Bureau of American
/// Ethnology annual report volume with an OCR'd `.txt` file (verified against
/// archive.org's `/metadata` API at the time this was wired). The manifest's
/// `url` for this source is the bare `https://archive.org` domain rather than
/// a per-item link, so the identifier is declared here rather than parsed.
const BAE_REPORTS_IDENTIFIER: &str = "annualreportofbu3019smit";

/// The `j-hagedorn/trilogy` GitHub repository owner/name, fetched for
/// `trilogy`. The manifest's `url` for this source is the bare repository
/// root, which has no directory/branch information, so those are declared
/// here rather than parsed.
const TRILOGY_OWNER: &str = "j-hagedorn";
const TRILOGY_REPO: &str = "trilogy";
/// Verified as the repository's default branch at the time this was wired.
const TRILOGY_BRANCH: &str = "master";
/// The repository directory holding the ATU + Thompson Motif Index + tale
/// data tables (verified against the repository tree at the time this was
/// wired); `docs`/`src`/`funs`/`ontologies` hold code and documentation, not
/// data.
const TRILOGY_DATA_DIR: &str = "data";

/// Why fetch dispatch for a source did not produce works.
pub(crate) enum DispatchOutcome {
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

/// Which fetcher family a ledger source id maps to, or why none does. Pure
/// (no I/O): parses whatever the family needs out of `entry`'s declared URL,
/// but never performs a request. See the module doc comment for why this is
/// factored out of [`fetch_for_source`].
enum Route {
    /// Search Gutendex and fetch the most popular matching candidate.
    GutendexSearch(SearchBy),
    /// Fetch one Gutendex ebook by its Project Gutenberg id.
    GutendexEbook(u64),
    /// Page a `HuggingFace` datasets-server dataset split.
    Huggingface(DatasetSpec),
    /// Fetch one archive.org item's text file.
    Archive { identifier: &'static str },
    /// List and download a GitHub repository directory's text-like data files.
    Git(RepoDirSpec),
    /// Run a SPARQL query against an endpoint and render the result rows.
    Sparql {
        endpoint: &'static str,
        query: &'static str,
        title: &'static str,
    },
    /// Resolve and download one file from a Zenodo record.
    Zenodo { record_id: u64 },
    /// No fetcher family is wired up (or configured) for this source id.
    NoFetcher(&'static str),
}

/// Resolve `entry`'s ledger source id to a [`Route`].
///
/// # Errors
///
/// Returns [`AcquireError::InvalidInput`] if the family routed to needs a
/// value parsed from `entry.url` (a Gutenberg ebook id) and that URL does not
/// parse.
fn route(entry: &SourceEntry) -> Result<Route, AcquireError> {
    match entry.id.as_str() {
        "polti" => Ok(Route::GutendexSearch(SearchBy::Query(
            "polti dramatic situations".to_owned(),
        ))),
        "pg_key_works" => Ok(Route::GutendexSearch(SearchBy::Topic(
            "mythology".to_owned(),
        ))),
        "child_ballads" => {
            // `classify_entry`'s `entry.url.is_none()` guard filters URL-less ~keep
            // sources before dispatch, so a dispatched source always has a URL; ~keep
            // `unwrap_or_default` yields `""` only in the unreachable case, ~keep
            // which `parse_ebook_id` then rejects as a parse error. ~keep
            let url = entry.url.as_deref().unwrap_or_default();
            Ok(Route::GutendexEbook(gutendex::parse_ebook_id(url)?))
        }
        "gutenberg_english" => Ok(Route::Huggingface(DatasetSpec {
            dataset: "sedthh/gutenberg_english".to_owned(),
            config: "default".to_owned(),
            split: "train".to_owned(),
            text_column: "TEXT".to_owned(),
        })),
        "pg19" => Ok(Route::Huggingface(DatasetSpec {
            dataset: "deepmind/pg19".to_owned(),
            config: "default".to_owned(),
            split: "train".to_owned(),
            text_column: "text".to_owned(),
        })),
        "bae_reports" => Ok(Route::Archive {
            identifier: BAE_REPORTS_IDENTIFIER,
        }),
        "trilogy" => Ok(Route::Git(RepoDirSpec {
            owner: TRILOGY_OWNER.to_owned(),
            repo: TRILOGY_REPO.to_owned(),
            branch: TRILOGY_BRANCH.to_owned(),
            dir: TRILOGY_DATA_DIR.to_owned(),
        })),
        "bag_of_tales" => {
            let url = entry.url.as_deref().unwrap_or_default();
            Ok(Route::Zenodo {
                record_id: zenodo::parse_record_id(url)?,
            })
        }
        "wikidata_myth" => Ok(Route::Sparql {
            endpoint: sparql::WIKIDATA_ENDPOINT,
            query: sparql::WIKIDATA_MYTH_QUERY,
            title: "Wikidata mythology subset",
        }),
        "dbpedia_myth" => Ok(Route::Sparql {
            endpoint: sparql::DBPEDIA_ENDPOINT,
            query: sparql::DBPEDIA_MYTH_QUERY,
            title: "DBpedia Deity/MythologicalFigure abstracts",
        }),
        "iapsop" => Ok(Route::NoFetcher(
            "plain HTTP site with no structured API; no generic HTML fetcher",
        )),
        _ => Ok(Route::NoFetcher("no fetcher wired up for this source id")),
    }
}

/// Dispatch one ledger source to its fetcher family, by source id.
///
/// See [`route`] for the pure id → fetcher-family mapping this executes.
pub(crate) async fn fetch_for_source(
    client: &reqwest::Client,
    store: &storage::BlobStore,
    entry: &SourceEntry,
    limit: Option<usize>,
    retrieved: &str,
) -> Result<Vec<FetchedWork>, DispatchOutcome> {
    let ctx = FetchContext::new(
        client,
        store,
        storage::StoragePrefix::from(entry.namespace),
        CacheMode::Enabled,
    );
    match route(entry)? {
        Route::GutendexSearch(by) => {
            let work = fetch_first_gutendex_search(&ctx, by, retrieved).await?;
            Ok(vec![work])
        }
        Route::GutendexEbook(pg_id) => {
            let work = gutendex::fetch(&ctx, pg_id, None, None, retrieved).await?;
            Ok(vec![work])
        }
        Route::Huggingface(spec) => fetch_huggingface(&ctx, spec, limit, retrieved).await,
        Route::Archive { identifier } => {
            let work = archive::fetch(&ctx, identifier, None, retrieved).await?;
            Ok(vec![work])
        }
        Route::Git(spec) => {
            let effective_limit = limit.unwrap_or(git::DEFAULT_LIMIT);
            let works = git::fetch_dir(&ctx, &spec, effective_limit, retrieved).await?;
            Ok(works)
        }
        Route::Sparql {
            endpoint,
            query,
            title,
        } => {
            let work =
                sparql::fetch(&ctx, endpoint, query, Some(title.to_owned()), retrieved).await?;
            Ok(vec![work])
        }
        Route::Zenodo { record_id } => {
            let work = zenodo::fetch(&ctx, record_id, None, retrieved).await?;
            Ok(vec![work])
        }
        Route::NoFetcher(reason) => Err(DispatchOutcome::NoFetcher { reason }),
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
/// works are raw Project Gutenberg pages and need [`super::normalize::strip_gutenberg`]
/// before paragraph cleanup; `HuggingFace` rows are already pre-cleaned dataset
/// text with no PG boilerplate to strip, so only
/// [`super::normalize::clean_full_text`] runs.
pub(crate) fn normalize_for_family(entry: &SourceEntry, work: &FetchedWork) -> String {
    let is_gutendex_family = GUTENDEX_FAMILY_SOURCE_IDS.contains(&entry.id.as_str());
    if is_gutendex_family {
        let (stripped, _was_stripped) = super::normalize::strip_gutenberg(&work.full_text);
        super::normalize::clean_full_text(&stripped)
    } else {
        super::normalize::clean_full_text(&work.full_text)
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

    /// Regression guard for the refactor this file exists to perform: every
    /// source that classifies as dispatchable under `BuildShip` must still
    /// resolve to something other than a bare parse error — i.e. `route`
    /// never panics or errors for a well-formed dispatchable ledger entry.
    #[test]
    fn route_succeeds_for_every_dispatchable_ship_source() {
        let ledger = ledger();
        for entry in crate::acquire::dispatchable_entries(
            &ledger,
            None,
            crate::acquire::AcquireMode::BuildShip,
        ) {
            route(entry).unwrap_or_else(|error| panic!("{}: {error}", entry.id));
        }
    }

    /// The dispatch point of this family being wired: `bae_reports` now
    /// resolves to a real fetcher (the archive.org family), not
    /// [`Route::NoFetcher`].
    #[test]
    fn route_resolves_bae_reports_to_the_archive_family_with_its_identifier() {
        let ledger = ledger();
        let entry = ledger.get("bae_reports").expect("declared");
        let resolved = route(entry).expect("bae_reports url parses");
        assert!(matches!(
            resolved,
            Route::Archive { identifier } if identifier == BAE_REPORTS_IDENTIFIER
        ));
    }

    /// The dispatch point of this family being wired: `trilogy` now resolves
    /// to a real fetcher (the git family), not [`Route::NoFetcher`].
    #[test]
    fn route_resolves_trilogy_to_the_git_family_with_the_verified_data_dir() {
        let ledger = ledger();
        let entry = ledger.get("trilogy").expect("declared");
        let resolved = route(entry).expect("trilogy url parses");
        match resolved {
            Route::Git(spec) => {
                assert_eq!(spec.owner, TRILOGY_OWNER);
                assert_eq!(spec.repo, TRILOGY_REPO);
                assert_eq!(spec.branch, TRILOGY_BRANCH);
                assert_eq!(spec.dir, TRILOGY_DATA_DIR);
            }
            _ => panic!("expected Route::Git for 'trilogy'"),
        }
    }

    /// The dispatch point of this family being wired: `wikidata_myth` and
    /// `dbpedia_myth` now resolve to a real fetcher (the sparql family), not
    /// [`Route::NoFetcher`].
    #[test]
    fn route_resolves_wikidata_myth_and_dbpedia_myth_to_the_sparql_family() {
        let ledger = ledger();
        for id in ["wikidata_myth", "dbpedia_myth"] {
            let entry = ledger.get(id).expect("declared");
            let resolved = route(entry).unwrap_or_else(|error| panic!("{id}: {error}"));
            assert!(
                matches!(resolved, Route::Sparql { .. }),
                "'{id}' must route to the sparql family"
            );
        }
    }

    /// The dispatch point of this family being wired: `bag_of_tales` now
    /// resolves to a real fetcher (the zenodo family), not
    /// [`Route::NoFetcher`].
    #[test]
    fn route_resolves_bag_of_tales_to_the_zenodo_family_with_its_record_id() {
        let ledger = ledger();
        let entry = ledger.get("bag_of_tales").expect("declared");
        let resolved = route(entry).expect("bag_of_tales url parses");
        assert!(matches!(
            resolved,
            Route::Zenodo {
                record_id: 6_575_263
            }
        ));
    }
}
