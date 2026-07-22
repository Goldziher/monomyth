//! Per-family fetchers: Gutendex (Project Gutenberg discovery + download),
//! `HuggingFace` datasets-server (paginated JSON rows), and archive.org
//! (metadata + file download).
//!
//! Each family has its own request shape, pagination scheme, and result
//! mapping, and the dispatch table in [`crate::acquire::dispatch`] routes to
//! them by ledger source id rather than by any shared trait method. A
//! `Fetcher` trait would buy dynamic dispatch we do not need — the dispatch
//! table already knows statically which family a given source id belongs to
//! — so this module favors free functions per family over a trait with many
//! implementors called through one interface.

pub(crate) mod archive;
pub(crate) mod gutendex;
pub(crate) mod huggingface;

/// A single fetched work, mapping 1:1 onto [`crate::IngestInput`]'s
/// provenance-bearing fields.
#[derive(Debug, Clone)]
pub struct FetchedWork {
    /// The clean (or dataset-provided) full text, before pipeline normalization.
    pub full_text: String,
    /// The URL the text was fetched from.
    pub url: String,
    /// The `sha256:<hex>` checksum of the raw fetched bytes.
    pub checksum: String,
    /// The retrieval date (`YYYY-MM-DD`), injected by the caller.
    pub retrieved: String,
    /// A human-readable title, when the fetcher family provides one.
    pub title: Option<String>,
}
