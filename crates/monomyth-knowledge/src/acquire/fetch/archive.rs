//! archive.org fetcher — resolve an item's metadata to a text file and
//! download it. No Python precedent; this family did not exist in the
//! retired prototype.
//!
//! The rule for picking a file, kept deliberately simple: prefer the
//! caller-specified filename when given; otherwise pick the first file in the
//! metadata's `files` array whose name ends in `.txt`.
//!
//! Not yet dispatched from [`crate::acquire::mod`]: the manifest declares no
//! specific archive.org identifier for `bae_reports`, the one ledger source
//! that would use this fetcher, so wiring one in without a real identifier
//! would risk a silent 404 against a made-up one. The fetcher itself is real
//! and unit-tested; only its call site is pending.
#![allow(
    dead_code,
    reason = "fetcher is real and tested; dispatch is pending a real archive.org identifier for bae_reports"
)]

use serde::Deserialize;

use crate::acquire::error::AcquireError;
use crate::acquire::fetch::FetchedWork;
use crate::acquire::http::{self, CacheMode};

/// The archive.org item metadata response, narrowed to what this fetcher uses.
#[derive(Debug, Deserialize)]
struct Metadata {
    #[serde(default)]
    files: Vec<FileEntry>,
}

/// One file entry in an item's metadata.
#[derive(Debug, Deserialize)]
struct FileEntry {
    name: String,
}

/// Fetch metadata for `identifier`, pick a text file (`filename` when given,
/// else the first `.txt`-suffixed file), and download it.
///
/// # Errors
///
/// Returns [`AcquireError::Http`]/[`AcquireError::Json`] if the metadata or
/// file request fails, or [`AcquireError::InvalidInput`] if no matching text
/// file is found in the item's metadata.
pub(crate) async fn fetch(
    client: &reqwest::Client,
    identifier: &str,
    filename: Option<&str>,
    retrieved: &str,
    cache_mode: CacheMode,
) -> Result<FetchedWork, AcquireError> {
    let metadata_url = format!("https://archive.org/metadata/{identifier}");
    let metadata: Metadata = http::get_json(client, &metadata_url, cache_mode).await?;

    let resolved_filename =
        pick_text_file(&metadata, filename).ok_or_else(|| AcquireError::InvalidInput {
            what: "archive.org text file",
            value: identifier.to_owned(),
        })?;

    let download_url = format!("https://archive.org/download/{identifier}/{resolved_filename}");
    let (text, raw_bytes) = http::get_text(client, &download_url, cache_mode).await?;
    let checksum = http::sha256_prefixed(&raw_bytes);

    Ok(FetchedWork {
        full_text: text,
        url: download_url,
        checksum,
        retrieved: retrieved.to_owned(),
        title: None,
    })
}

/// Pick the file to download: the caller-specified `filename` when present
/// (regardless of extension — the caller knows what they asked for),
/// otherwise the first `.txt`-suffixed file in `metadata.files`.
fn pick_text_file(metadata: &Metadata, filename: Option<&str>) -> Option<String> {
    if let Some(filename) = filename {
        return Some(filename.to_owned());
    }
    metadata
        .files
        .iter()
        .find(|file| {
            std::path::Path::new(&file.name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("txt"))
        })
        .map(|file| file.name.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_text_file_prefers_caller_specified_filename() {
        let metadata = Metadata {
            files: vec![FileEntry {
                name: "other.txt".to_owned(),
            }],
        };
        assert_eq!(
            pick_text_file(&metadata, Some("wanted.txt")),
            Some("wanted.txt".to_owned())
        );
    }

    #[test]
    fn pick_text_file_falls_back_to_first_txt_suffixed_file() {
        let metadata = Metadata {
            files: vec![
                FileEntry {
                    name: "cover.jpg".to_owned(),
                },
                FileEntry {
                    name: "bae_report_1.txt".to_owned(),
                },
                FileEntry {
                    name: "bae_report_2.txt".to_owned(),
                },
            ],
        };
        assert_eq!(
            pick_text_file(&metadata, None),
            Some("bae_report_1.txt".to_owned())
        );
    }

    #[test]
    fn pick_text_file_returns_none_with_no_txt_file_and_no_override() {
        let metadata = Metadata {
            files: vec![FileEntry {
                name: "cover.jpg".to_owned(),
            }],
        };
        assert_eq!(pick_text_file(&metadata, None), None);
    }
}
