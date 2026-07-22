//! DOI/Zenodo dataset fetcher — resolve a Zenodo record's file listing via
//! its JSON API, download a file, and (when it is a zip archive) extract its
//! text-like member files.
//!
//! Ports no Python precedent; the retired prototype declared `bag_of_tales`
//! (a Zenodo DOI) but never fetched it. The metadata and file requests reuse
//! the shared HTTP transport; only reading a downloaded archive's members
//! pulls in a dependency (`zip`), gated the same as every other `acquire`
//! dependency — many Zenodo research-dataset records package their files as
//! a single zip, so this is a generally useful capability, not a
//! `bag_of_tales`-specific special case.

use std::io::{Cursor, Read as _};

use serde::Deserialize;

use crate::acquire::error::AcquireError;
use crate::acquire::fetch::FetchedWork;
use crate::acquire::http::{self, FetchContext};

/// File extensions extracted from a downloaded zip archive's members;
/// everything else (binaries, source files, project files) is skipped.
const TEXT_LIKE_EXTENSIONS: [&str; 5] = ["csv", "tsv", "json", "md", "txt"];

/// A Zenodo record's file listing, narrowed to what this fetcher uses.
#[derive(Debug, Deserialize)]
struct RecordResponse {
    files: Vec<RecordFile>,
}

/// One declared file in a Zenodo record.
#[derive(Debug, Deserialize)]
struct RecordFile {
    key: String,
    links: FileLinks,
}

/// A record file's download link.
#[derive(Debug, Deserialize)]
struct FileLinks {
    #[serde(rename = "self")]
    self_: String,
}

/// Parse a Zenodo record id from the trailing `zenodo.<id>` DOI segment of a
/// `https://doi.org/10.5281/zenodo.<id>` URL.
///
/// # Errors
///
/// Returns [`AcquireError::InvalidInput`] if the URL has no trailing
/// `zenodo.<digits>` segment.
pub(crate) fn parse_record_id(url: &str) -> Result<u64, AcquireError> {
    url.rsplit('/')
        .find(|segment| !segment.is_empty())
        .and_then(|segment| segment.strip_prefix("zenodo."))
        .and_then(|digits| digits.parse::<u64>().ok())
        .ok_or_else(|| AcquireError::InvalidInput {
            what: "zenodo record id",
            value: url.to_owned(),
        })
}

/// Pick the file to download: the caller-specified `key` when present,
/// otherwise the first declared file.
fn pick_file<'a>(files: &'a [RecordFile], key: Option<&str>) -> Option<&'a RecordFile> {
    if let Some(key) = key {
        return files.iter().find(|file| file.key == key);
    }
    files.first()
}

/// Fetch record `record_id`'s metadata, pick a file (`file_key` when given,
/// else the first declared file), download it, and — if its name ends in
/// `.zip` — extract its text-like member files (see
/// [`TEXT_LIKE_EXTENSIONS`]); otherwise decode the downloaded bytes as text
/// directly.
///
/// # Errors
///
/// Returns [`AcquireError::Http`]/[`AcquireError::Json`] if the metadata or
/// file request fails, [`AcquireError::InvalidInput`] if the record declares
/// no matching file, or [`AcquireError::Zip`] if a `.zip` download cannot be
/// read as a zip archive.
pub(crate) async fn fetch(
    ctx: &FetchContext<'_>,
    record_id: u64,
    file_key: Option<&str>,
    retrieved: &str,
) -> Result<FetchedWork, AcquireError> {
    let record_url = format!("https://zenodo.org/api/records/{record_id}");
    let record: RecordResponse = http::get_json(ctx, &record_url).await?;

    let file = pick_file(&record.files, file_key).ok_or_else(|| AcquireError::InvalidInput {
        what: "zenodo record file",
        value: record_id.to_string(),
    })?;

    let raw_bytes = http::get_bytes(ctx, &file.links.self_).await?;
    let checksum = http::sha256_prefixed(&raw_bytes);
    let full_text = if is_zip(&file.key) {
        extract_zip_text(&raw_bytes)?
    } else {
        String::from_utf8_lossy(&raw_bytes).into_owned()
    };

    Ok(FetchedWork {
        full_text,
        url: file.links.self_.clone(),
        checksum,
        retrieved: retrieved.to_owned(),
        title: Some(file.key.clone()),
    })
}

/// Whether `filename` names a zip archive.
fn is_zip(filename: &str) -> bool {
    std::path::Path::new(filename)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

/// Whether `filename`'s extension is one of [`TEXT_LIKE_EXTENSIONS`].
fn is_text_like(filename: &str) -> bool {
    std::path::Path::new(filename)
        .extension()
        .is_some_and(|extension| {
            TEXT_LIKE_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

/// Extract every text-like member of a zip archive (see
/// [`TEXT_LIKE_EXTENSIONS`]) and concatenate them as
/// `"== <member name> ==\n<content>\n\n"` blocks, in archive order.
fn extract_zip_text(bytes: &[u8]) -> Result<String, AcquireError> {
    let mut archive =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|source| AcquireError::Zip {
            what: "opening zip archive",
            source,
        })?;

    let mut out = String::new();
    for index in 0..archive.len() {
        let mut member = archive
            .by_index(index)
            .map_err(|source| AcquireError::Zip {
                what: "reading zip member",
                source,
            })?;
        if !member.is_file() || !is_text_like(member.name()) {
            continue;
        }
        let name = member.name().to_owned();
        let mut content = String::new();
        member
            .read_to_string(&mut content)
            .map_err(|error| AcquireError::Zip {
                what: "decoding zip member as UTF-8",
                source: zip::result::ZipError::Io(error),
            })?;
        out.push_str("== ");
        out.push_str(&name);
        out.push_str(" ==\n");
        out.push_str(&content);
        out.push_str("\n\n");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn record_file(key: &str, url: &str) -> RecordFile {
        RecordFile {
            key: key.to_owned(),
            links: FileLinks {
                self_: url.to_owned(),
            },
        }
    }

    #[test]
    fn parse_record_id_extracts_the_trailing_zenodo_segment() {
        assert_eq!(
            parse_record_id("https://doi.org/10.5281/zenodo.6575263").expect("parses"),
            6_575_263
        );
    }

    #[test]
    fn parse_record_id_rejects_a_url_with_no_zenodo_segment() {
        let error =
            parse_record_id("https://doi.org/10.5281/not-zenodo.123").expect_err("must fail");
        match error {
            AcquireError::InvalidInput { what, value } => {
                assert_eq!(what, "zenodo record id");
                assert_eq!(value, "https://doi.org/10.5281/not-zenodo.123");
            }
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn parse_record_id_rejects_a_non_numeric_zenodo_segment() {
        let error = parse_record_id("https://doi.org/10.5281/zenodo.abc").expect_err("must fail");
        assert!(matches!(error, AcquireError::InvalidInput { .. }));
    }

    #[test]
    fn pick_file_prefers_the_caller_specified_key() {
        let files = vec![
            record_file("a.zip", "https://example.org/a.zip"),
            record_file("b.zip", "https://example.org/b.zip"),
        ];
        let picked = pick_file(&files, Some("b.zip")).expect("b.zip is declared");
        assert_eq!(picked.key, "b.zip");
    }

    #[test]
    fn pick_file_falls_back_to_the_first_declared_file() {
        let files = vec![
            record_file("a.zip", "https://example.org/a.zip"),
            record_file("b.zip", "https://example.org/b.zip"),
        ];
        let picked = pick_file(&files, None).expect("at least one file is declared");
        assert_eq!(picked.key, "a.zip");
    }

    #[test]
    fn pick_file_returns_none_for_an_empty_file_list() {
        assert!(pick_file(&[], None).is_none());
    }

    #[test]
    fn is_zip_and_is_text_like_classify_by_extension() {
        assert!(is_zip("trilogy-v1.1.zip"));
        assert!(!is_zip("data.csv"));
        assert!(is_text_like("atu_df.csv"));
        assert!(!is_text_like("cover.jpg"));
    }

    /// Build an in-memory zip with one text-like member and one skipped
    /// member, so [`extract_zip_text`] is exercised end-to-end with no
    /// network and no fixture file on disk.
    #[test]
    fn extract_zip_text_concatenates_only_text_like_members_in_archive_order() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();

        writer.start_file("data.csv", options).expect("start_file");
        writer.write_all(b"a,b\n1,2\n").expect("write csv member");
        writer.start_file("cover.jpg", options).expect("start_file");
        writer
            .write_all(b"\x00\x01binary")
            .expect("write jpg member");
        let cursor = writer.finish().expect("finish archive");
        let bytes = cursor.into_inner();

        let text = extract_zip_text(&bytes).expect("archive reads back");
        assert_eq!(text, "== data.csv ==\na,b\n1,2\n\n\n");
    }
}
