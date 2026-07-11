//! Error model for the corpus acquisition pipeline.

use thiserror::Error;

/// Errors raised by the `acquire` module: fetch, normalize, and dispatch
/// failures that occur before text ever reaches [`crate::Knowledge::ingest`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AcquireError {
    /// An HTTP request failed after exhausting all retries.
    #[error("GET {url} failed after {attempts} attempt(s)")]
    Http {
        /// The URL that could not be fetched.
        url: String,
        /// How many attempts were made.
        attempts: u32,
        /// The underlying transport error from the last attempt.
        #[source]
        source: reqwest::Error,
    },

    /// A response body could not be decoded as JSON.
    #[error("failed to decode JSON response from {url}")]
    Json {
        /// The URL whose response failed to decode.
        url: String,
        /// The underlying decode error.
        #[source]
        source: serde_json::Error,
    },

    /// Reading from or writing to the on-disk fetch cache failed.
    #[error("cache IO failed for {path}")]
    Cache {
        /// The cache file path involved.
        path: String,
        /// The underlying IO error.
        #[source]
        source: std::io::Error,
    },

    /// A response body exceeded the maximum accepted size, guarding against an
    /// oversized or decompression-bomb response buffering unbounded memory.
    #[error("response from {url} exceeded the {limit}-byte size cap")]
    TooLarge {
        /// The URL whose response was too large.
        url: String,
        /// The byte cap that was exceeded.
        limit: u64,
    },

    /// A value taken from a declared source (e.g. a ledger URL) failed a
    /// boundary validation check, such as parsing a numeric id.
    #[error("invalid {what}: {value:?}")]
    InvalidInput {
        /// What was being parsed or validated.
        what: &'static str,
        /// The offending value.
        value: String,
    },

    /// The ledger declares a source with no fetcher wired up yet, or with a
    /// fetcher that needs configuration (e.g. an archive.org identifier) not
    /// yet supplied.
    #[error("no fetcher available for source '{source_id}': {reason}")]
    NoFetcher {
        /// The ledger source id.
        source_id: String,
        /// Why no fetcher is available.
        reason: &'static str,
    },
}
