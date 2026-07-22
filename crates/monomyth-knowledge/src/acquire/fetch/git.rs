//! GitHub repository data-file fetcher — list a directory via the GitHub
//! Contents API and download each matching file's raw bytes.
//!
//! No git clone and no tarball extraction: the Contents API returns each
//! file's `download_url` (a `raw.githubusercontent.com` link), so every
//! request in this family is a plain HTTP GET through the shared transport —
//! no new dependency, no local git binary invoked. Ports no Python
//! precedent; the retired prototype declared `trilogy` but never fetched it.

use serde::Deserialize;

use crate::acquire::error::AcquireError;
use crate::acquire::fetch::FetchedWork;
use crate::acquire::http::{self, FetchContext};

/// Default number of files fetched from a directory listing when the caller
/// supplies no `limit`, so an unattended run cannot pull every (some
/// multi-megabyte) data file in the directory in one call.
pub(crate) const DEFAULT_LIMIT: usize = 3;

/// File extensions treated as fetchable text/data. Everything else (build
/// scripts, project files, binary `.graphml` graphs, images) is skipped.
const TEXT_LIKE_EXTENSIONS: [&str; 5] = ["csv", "tsv", "json", "md", "txt"];

/// Which repository directory to list and download.
#[derive(Debug, Clone)]
pub(crate) struct RepoDirSpec {
    /// The repository owner (user or organization).
    pub owner: String,
    /// The repository name.
    pub repo: String,
    /// The branch (or tag/commit) to read the directory listing from.
    pub branch: String,
    /// The directory path within the repository.
    pub dir: String,
}

/// One entry in a GitHub Contents API directory listing, narrowed to what
/// this fetcher uses.
#[derive(Debug, Deserialize)]
struct ContentEntry {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    download_url: Option<String>,
}

/// List `spec.dir` and download up to `limit` of its text-like files (see
/// [`TEXT_LIKE_EXTENSIONS`]), in listing order.
///
/// # Errors
///
/// Returns [`AcquireError::Http`]/[`AcquireError::Json`] if the directory
/// listing or a file download fails.
pub(crate) async fn fetch_dir(
    ctx: &FetchContext<'_>,
    spec: &RepoDirSpec,
    limit: usize,
    retrieved: &str,
) -> Result<Vec<FetchedWork>, AcquireError> {
    let listing_url = contents_url(spec);
    let entries: Vec<ContentEntry> = http::get_json(ctx, &listing_url).await?;

    let mut works = Vec::new();
    for entry in entries.iter().filter(|entry| is_fetchable(entry)) {
        if works.len() >= limit {
            break;
        }
        // `is_fetchable` requires `download_url.is_some()`, so this always ~keep
        // matches; `if let` avoids a second `unwrap`-shaped assumption here. ~keep
        let Some(download_url) = entry.download_url.clone() else {
            continue;
        };
        let (text, raw_bytes) = http::get_text(ctx, &download_url).await?;
        let checksum = http::sha256_prefixed(&raw_bytes);
        works.push(FetchedWork {
            full_text: text,
            url: download_url,
            checksum,
            retrieved: retrieved.to_owned(),
            title: Some(entry.name.clone()),
        });
    }
    Ok(works)
}

/// Build the GitHub Contents API request URL for `spec`'s directory.
fn contents_url(spec: &RepoDirSpec) -> String {
    format!(
        "https://api.github.com/repos/{}/{}/contents/{}?ref={}",
        spec.owner, spec.repo, spec.dir, spec.branch
    )
}

/// Whether a directory entry is a downloadable text-like file.
fn is_fetchable(entry: &ContentEntry) -> bool {
    entry.kind == "file" && entry.download_url.is_some() && is_text_like(&entry.name)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> RepoDirSpec {
        RepoDirSpec {
            owner: "j-hagedorn".to_owned(),
            repo: "trilogy".to_owned(),
            branch: "master".to_owned(),
            dir: "data".to_owned(),
        }
    }

    #[test]
    fn contents_url_includes_owner_repo_dir_and_branch() {
        assert_eq!(
            contents_url(&spec()),
            "https://api.github.com/repos/j-hagedorn/trilogy/contents/data?ref=master"
        );
    }

    #[test]
    fn is_text_like_accepts_csv_and_rejects_graphml() {
        assert!(is_text_like("atu_df.csv"));
        assert!(is_text_like("propp.csv"));
        assert!(!is_text_like("motif_graph.graphml"));
        assert!(!is_text_like("trilogy.Rproj"));
    }

    #[test]
    fn is_fetchable_requires_file_kind_and_a_download_url() {
        let file = ContentEntry {
            name: "atu_df.csv".to_owned(),
            kind: "file".to_owned(),
            download_url: Some("https://raw.githubusercontent.com/x".to_owned()),
        };
        assert!(is_fetchable(&file));

        let dir = ContentEntry {
            name: "process_files".to_owned(),
            kind: "dir".to_owned(),
            download_url: None,
        };
        assert!(!is_fetchable(&dir));

        let non_text = ContentEntry {
            name: "motif_graph.graphml".to_owned(),
            kind: "file".to_owned(),
            download_url: Some("https://raw.githubusercontent.com/x".to_owned()),
        };
        assert!(!is_fetchable(&non_text));
    }
}
