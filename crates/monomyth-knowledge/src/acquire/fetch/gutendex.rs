//! Project Gutenberg fetcher — discover PD works by topic/search via Gutendex
//! and fetch clean plain text. Ports the retired Python prototype's
//! `fetch/gutenberg.py`.
//!
//! `copyright=false` on Gutendex is PG's own US-public-domain determination,
//! so it is our ship-safe gate: we only ever accept works PG marks
//! non-copyright. Gutendex returns `null` when copyright status is unknown —
//! that must **not** pass the gate, only an explicit `false` does.

use serde::Deserialize;

use crate::acquire::error::AcquireError;
use crate::acquire::fetch::FetchedWork;
use crate::acquire::http::{self, FetchContext};

/// Hard cap on paginated Gutendex search requests, so a pathological result
/// set cannot loop indefinitely.
const MAX_SEARCH_PAGES: usize = 5;

/// A candidate Gutenberg work returned by [`search_topic`] / [`search_query`].
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    /// The Project Gutenberg ebook id.
    pub pg_id: u64,
    /// The work's title.
    pub title: String,
    /// Gutendex's download-count popularity signal.
    pub download_count: u64,
    /// The `text/plain` (non-zip) download URL, when the work has one.
    pub text_url: Option<String>,
    /// Author names, as reported by Gutendex.
    pub authors: Vec<String>,
}

/// How a Gutendex search is filtered: by topic bookshelf or by free-text
/// search. Mutually exclusive by construction.
#[derive(Debug, Clone)]
pub(crate) enum SearchBy {
    /// Filter by Gutendex `topic` (a bookshelf/subject tag).
    Topic(String),
    /// Filter by Gutendex free-text `search`.
    Query(String),
}

/// A single page of the Gutendex `/books` response.
#[derive(Debug, Deserialize)]
struct BooksPage {
    results: Vec<BookRow>,
    next: Option<String>,
}

/// A single book row from the Gutendex API.
#[derive(Debug, Deserialize)]
struct BookRow {
    id: u64,
    title: String,
    #[serde(default)]
    download_count: u64,
    #[serde(default)]
    authors: Vec<AuthorRow>,
    #[serde(default)]
    formats: std::collections::BTreeMap<String, String>,
    /// `Some(false)` is PD; `Some(true)` is in-copyright; `None` is unknown —
    /// only `Some(false)` passes the ship-safe gate.
    copyright: Option<bool>,
}

/// An author entry in a Gutendex book row.
#[derive(Debug, Deserialize)]
struct AuthorRow {
    #[serde(default)]
    name: String,
}

/// Search Gutendex for PD (`copyright=false`) English-language candidates
/// under `by`, paginating up to [`MAX_SEARCH_PAGES`] pages.
///
/// # Errors
///
/// Returns [`AcquireError::Http`]/[`AcquireError::Json`] if a page request
/// fails or fails to decode.
pub(crate) async fn search(
    ctx: &FetchContext<'_>,
    by: &SearchBy,
) -> Result<Vec<Candidate>, AcquireError> {
    let mut url = Some(first_page_url(by));
    let mut candidates = Vec::new();
    let mut pages_fetched = 0;

    while let Some(page_url) = url {
        if pages_fetched >= MAX_SEARCH_PAGES {
            break;
        }
        let page: BooksPage = http::get_json(ctx, &page_url).await?;
        pages_fetched += 1;

        candidates.extend(page.results.into_iter().filter_map(|row| {
            if row.copyright != Some(false) {
                return None;
            }
            let text_url = plain_text_url(&row.formats);
            Some(Candidate {
                pg_id: row.id,
                title: row.title,
                download_count: row.download_count,
                text_url,
                authors: row.authors.into_iter().map(|author| author.name).collect(),
            })
        }));

        url = page.next;
    }

    Ok(candidates)
}

/// Build the first Gutendex request URL for `by`.
fn first_page_url(by: &SearchBy) -> String {
    let filter = match by {
        SearchBy::Topic(topic) => format!("&topic={}", urlencode(topic)),
        SearchBy::Query(query) => format!("&search={}", urlencode(query)),
    };
    format!("https://gutendex.com/books?languages=en&copyright=false{filter}")
}

/// Percent-encode a query parameter value. Minimal — Gutendex query values in
/// this pipeline are plain ASCII words/phrases with spaces.
fn urlencode(value: &str) -> String {
    value.replace(' ', "%20")
}

/// Pick the format whose MIME type starts with `text/plain` and whose URL is
/// not a zip archive.
fn plain_text_url(formats: &std::collections::BTreeMap<String, String>) -> Option<String> {
    formats
        .iter()
        .find(|(mime, url)| {
            mime.starts_with("text/plain")
                && !std::path::Path::new(url)
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
        })
        .map(|(_, url)| url.clone())
}

/// Fetch one work's plain text and provenance.
///
/// `text_url` overrides the canonical `pgN.txt` URL when given (e.g. from a
/// [`Candidate`]); otherwise falls back to
/// `https://www.gutenberg.org/cache/epub/{pg_id}/pg{pg_id}.txt`.
///
/// # Errors
///
/// Returns [`AcquireError::Http`] if the download fails after retries.
pub(crate) async fn fetch(
    ctx: &FetchContext<'_>,
    pg_id: u64,
    text_url: Option<&str>,
    title: Option<String>,
    retrieved: &str,
) -> Result<FetchedWork, AcquireError> {
    let url = text_url.map_or_else(
        || format!("https://www.gutenberg.org/cache/epub/{pg_id}/pg{pg_id}.txt"),
        str::to_owned,
    );
    let (text, raw_bytes) = http::get_text(ctx, &url).await?;
    let checksum = http::sha256_prefixed(&raw_bytes);

    Ok(FetchedWork {
        full_text: text,
        url,
        checksum,
        retrieved: retrieved.to_owned(),
        title,
    })
}

/// Parse a Project Gutenberg ebook id from the trailing path segment of an
/// `https://www.gutenberg.org/ebooks/<id>` URL.
///
/// # Errors
///
/// Returns [`AcquireError::InvalidInput`] if the URL has no trailing numeric
/// segment.
pub(crate) fn parse_ebook_id(url: &str) -> Result<u64, AcquireError> {
    url.rsplit('/')
        .find(|segment| !segment.is_empty())
        .and_then(|segment| segment.parse::<u64>().ok())
        .ok_or_else(|| AcquireError::InvalidInput {
            what: "gutenberg ebook id",
            value: url.to_owned(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ebook_id_extracts_trailing_numeric_segment() {
        assert_eq!(
            parse_ebook_id("https://www.gutenberg.org/ebooks/44969").expect("parses"),
            44_969
        );
    }

    #[test]
    fn parse_ebook_id_rejects_a_non_numeric_trailing_segment() {
        let error =
            parse_ebook_id("https://www.gutenberg.org/ebooks/not-a-number").expect_err("must fail");
        match error {
            AcquireError::InvalidInput { what, value } => {
                assert_eq!(what, "gutenberg ebook id");
                assert_eq!(value, "https://www.gutenberg.org/ebooks/not-a-number");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn parse_ebook_id_skips_a_trailing_slash_to_find_the_numeric_segment() {
        assert_eq!(
            parse_ebook_id("https://www.gutenberg.org/ebooks/44969/").expect("parses"),
            44_969
        );
    }

    #[test]
    fn parse_ebook_id_rejects_a_url_with_no_numeric_segment_at_all() {
        let error = parse_ebook_id("https://www.gutenberg.org/ebooks/").expect_err("must fail");
        assert!(matches!(error, AcquireError::InvalidInput { .. }));
    }

    #[test]
    fn plain_text_url_prefers_text_plain_and_skips_zip() {
        let mut formats = std::collections::BTreeMap::new();
        formats.insert(
            "application/zip".to_owned(),
            "https://example.org/book.zip".to_owned(),
        );
        formats.insert(
            "text/plain; charset=us-ascii".to_owned(),
            "https://example.org/book.txt".to_owned(),
        );
        assert_eq!(
            plain_text_url(&formats),
            Some("https://example.org/book.txt".to_owned())
        );
    }

    #[test]
    fn plain_text_url_rejects_a_zip_suffixed_text_plain_entry() {
        let mut formats = std::collections::BTreeMap::new();
        formats.insert(
            "text/plain; charset=us-ascii".to_owned(),
            "https://example.org/book.txt.zip".to_owned(),
        );
        assert_eq!(plain_text_url(&formats), None);
    }

    #[test]
    fn plain_text_url_returns_none_with_no_matching_format() {
        let mut formats = std::collections::BTreeMap::new();
        formats.insert(
            "application/epub+zip".to_owned(),
            "https://example.org/book.epub".to_owned(),
        );
        assert_eq!(plain_text_url(&formats), None);
    }

    #[test]
    fn first_page_url_uses_topic_filter() {
        let url = first_page_url(&SearchBy::Topic("mythology".to_owned()));
        assert_eq!(
            url,
            "https://gutendex.com/books?languages=en&copyright=false&topic=mythology"
        );
    }

    #[test]
    fn first_page_url_uses_search_filter_with_encoded_spaces() {
        let url = first_page_url(&SearchBy::Query("polti dramatic situations".to_owned()));
        assert_eq!(
            url,
            "https://gutendex.com/books?languages=en&copyright=false&search=polti%20dramatic%20situations"
        );
    }
}
