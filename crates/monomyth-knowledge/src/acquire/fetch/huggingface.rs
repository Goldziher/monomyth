//! `HuggingFace` datasets-server fetcher — paginated JSON rows over a dataset
//! split, extracting one text column per row. No Python precedent; this
//! family did not exist in the retired prototype.
//!
//! Parametrized by dataset/config/split/column so both `gutenberg_english`
//! (`sedthh/gutenberg_english`) and `pg19` (`deepmind/pg19`) dispatch through
//! this one fetcher with different arguments.

use serde::Deserialize;
use serde_json::Value;

use crate::acquire::error::AcquireError;
use crate::acquire::fetch::FetchedWork;
use crate::acquire::http::{self, FetchContext};

/// Hard cap on rows requested per page; the datasets-server API rejects
/// larger page sizes, so caller-supplied lengths are clamped rather than
/// passed through unchecked.
const MAX_PAGE_LENGTH: u32 = 100;

/// A single page of the datasets-server `/rows` response.
#[derive(Debug, Deserialize)]
struct RowsPage {
    rows: Vec<RowEnvelope>,
}

/// One row entry, wrapping the actual row object under `row`.
#[derive(Debug, Deserialize)]
struct RowEnvelope {
    row: Value,
}

/// Which dataset split to page through and which column holds the text.
#[derive(Debug, Clone)]
pub(crate) struct DatasetSpec {
    /// The `HuggingFace` dataset id (e.g. `sedthh/gutenberg_english`).
    pub dataset: String,
    /// The dataset config name.
    pub config: String,
    /// The split name (e.g. `train`).
    pub split: String,
    /// The row-object field holding the text to ingest.
    pub text_column: String,
}

/// Clamp a caller-requested page length to [`MAX_PAGE_LENGTH`]. The
/// datasets-server API rejects larger page sizes, so a caller-supplied length
/// over the cap is clamped at this boundary rather than passed through and
/// left to fail server-side.
#[must_use]
pub(crate) fn clamp_page_length(requested: u32) -> u32 {
    requested.min(MAX_PAGE_LENGTH)
}

/// Fetch up to `limit` rows' text from `spec`, paging by `offset` in batches
/// of `requested_page_length` (clamped to [`MAX_PAGE_LENGTH`]) until a page
/// returns fewer rows than requested or `limit` total rows have been
/// collected.
///
/// # Errors
///
/// Returns [`AcquireError::Http`]/[`AcquireError::Json`] if a page request
/// fails, or [`AcquireError::InvalidInput`] if a row is missing the
/// configured `text_column`.
pub(crate) async fn fetch_rows(
    ctx: &FetchContext<'_>,
    spec: &DatasetSpec,
    limit: usize,
    requested_page_length: u32,
    retrieved: &str,
) -> Result<Vec<FetchedWork>, AcquireError> {
    let page_length = clamp_page_length(requested_page_length);
    let mut offset: u32 = 0;
    let mut works = Vec::new();

    loop {
        if works.len() >= limit {
            break;
        }
        let url = page_url(spec, offset, page_length);
        let page: RowsPage = http::get_json(ctx, &url).await?;
        let fetched_row_count = page.rows.len();

        for envelope in page.rows {
            if works.len() >= limit {
                break;
            }
            works.push(row_to_work(spec, &envelope.row, &url, retrieved)?);
        }

        if fetched_row_count < page_length as usize {
            break;
        }
        offset += page_length;
    }

    Ok(works)
}

/// Build the datasets-server `/rows` request URL for one page.
fn page_url(spec: &DatasetSpec, offset: u32, length: u32) -> String {
    format!(
        "https://datasets-server.huggingface.co/rows?dataset={}&config={}&split={}&offset={offset}&length={length}",
        spec.dataset, spec.config, spec.split
    )
}

/// Extract the configured text column from one row object and wrap it as a
/// [`FetchedWork`]. Since datasets-server rows are pre-cleaned dataset text
/// (not raw scraped Gutenberg pages), the checksum is taken over the row's
/// UTF-8 text bytes directly rather than a separately fetched raw byte
/// stream.
fn row_to_work(
    spec: &DatasetSpec,
    row: &Value,
    url: &str,
    retrieved: &str,
) -> Result<FetchedWork, AcquireError> {
    let text = row
        .get(&spec.text_column)
        .and_then(Value::as_str)
        .ok_or_else(|| AcquireError::InvalidInput {
            what: "huggingface row text column",
            value: spec.text_column.clone(),
        })?
        .to_owned();
    let checksum = http::sha256_prefixed(text.as_bytes());

    Ok(FetchedWork {
        full_text: text,
        url: url.to_owned(),
        checksum,
        retrieved: retrieved.to_owned(),
        title: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> DatasetSpec {
        DatasetSpec {
            dataset: "sedthh/gutenberg_english".to_owned(),
            config: "default".to_owned(),
            split: "train".to_owned(),
            text_column: "TEXT".to_owned(),
        }
    }

    #[test]
    fn clamp_page_length_passes_through_a_value_at_or_below_the_cap() {
        assert_eq!(clamp_page_length(50), 50);
        assert_eq!(clamp_page_length(100), 100);
    }

    #[test]
    fn clamp_page_length_clamps_a_value_over_the_cap() {
        assert_eq!(clamp_page_length(500), 100);
    }

    #[test]
    fn page_url_includes_dataset_config_split_offset_and_length() {
        let url = page_url(&spec(), 200, 100);
        assert_eq!(
            url,
            "https://datasets-server.huggingface.co/rows?dataset=sedthh/gutenberg_english&config=default&split=train&offset=200&length=100"
        );
    }

    #[test]
    fn row_to_work_extracts_the_configured_text_column() {
        let row = serde_json::json!({"TEXT": "Once upon a time.", "other": "ignored"});
        let work = row_to_work(&spec(), &row, "https://example.org/rows", "2026-07-11")
            .expect("row has the text column");
        assert_eq!(work.full_text, "Once upon a time.");
        assert_eq!(work.url, "https://example.org/rows");
        assert_eq!(work.retrieved, "2026-07-11");
        assert_eq!(work.title, None);
        assert_eq!(
            work.checksum,
            http::sha256_prefixed("Once upon a time.".as_bytes())
        );
    }

    #[test]
    fn row_to_work_rejects_a_row_missing_the_text_column() {
        let row = serde_json::json!({"other": "no text here"});
        let error = row_to_work(&spec(), &row, "https://example.org/rows", "2026-07-11")
            .expect_err("missing column must fail");
        match error {
            AcquireError::InvalidInput { what, value } => {
                assert_eq!(what, "huggingface row text column");
                assert_eq!(value, "TEXT");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }
}
