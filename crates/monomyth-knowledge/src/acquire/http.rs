//! Cached, retrying HTTP GET for corpus acquisition.
//!
//! Ports the retired Python prototype's `http.py`: a polite `User-Agent`, a
//! raw-bytes cache keyed by a hash of the URL (so re-runs are idempotent and
//! offline-capable) fronted by [`super::storage::BlobStore`] (ADR-0012),
//! retry with backoff, and a 30s timeout. Gzip is handled transparently by
//! reqwest's `gzip` feature rather than hand-rolled decompression.

use std::fmt::Write as _;
use std::time::Duration;

use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::acquire::error::AcquireError;
use crate::acquire::storage;

/// Identifies this pipeline to remote servers as a polite, identifiable client.
const USER_AGENT: &str = "monomyth-corpus/0.1 (+https://github.com/monomyth; research/indexing)";

/// Request timeout for every attempt.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Number of attempts made before giving up.
const MAX_ATTEMPTS: u32 = 3;

/// Base multiplier (seconds) for the linear backoff between retries: attempt
/// `n` (1-indexed) sleeps `1.5 * n` seconds before the next try.
const BACKOFF_BASE_SECONDS: f64 = 1.5;

/// Maximum response body accepted from a single fetch. Guards against an
/// oversized or decompression-bomb response buffering unbounded memory: the
/// body is accumulated in bounded chunks and the fetch is aborted the moment
/// the *decompressed* total crosses this cap. Deliberately generous — the
/// largest legitimate corpus text (a full Project Gutenberg book) is a few MB —
/// while still bounded.
const MAX_RESPONSE_BYTES: u64 = 256 * 1024 * 1024;

/// Whether a fetch reads/writes the on-disk cache.
///
/// `Disabled` is not yet reachable from [`crate::acquire::build_corpus`] (the
/// orchestrator always caches), but is part of the deliberate public shape
/// for a future `--no-cache` escape hatch and is exercised directly in tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "reserved for a future --no-cache escape hatch; exercised in tests"
)]
pub(crate) enum CacheMode {
    /// Read a cache hit if present; write a cache miss after fetching.
    Enabled,
    /// Always fetch over the network; never read or write the cache.
    Disabled,
}

/// Lowercase-hex-encode `bytes` into `out`, appending in place.
pub(crate) fn write_hex(out: &mut String, bytes: &[u8]) {
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
}

/// Hash `bytes` with SHA-256 and return the `"sha256:<hex>"` checksum string
/// used as content provenance (ADR-0005).
#[must_use]
pub(crate) fn sha256_prefixed(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(7 + digest.len() * 2);
    out.push_str("sha256:");
    write_hex(&mut out, &digest);
    out
}

/// Hash `bytes` with SHA-256 and truncate to the first 20 lowercase-hex
/// characters — the cache-key hash segment shared by
/// [`super::storage::cache_key`] so blob-store keys stay byte-for-byte stable
/// with the hash scheme this module has always used.
#[must_use]
pub(crate) fn sha256_hex20(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    write_hex(&mut hex, &digest);
    hex.truncate(20);
    hex
}

/// Backoff duration before the attempt after `attempt` (1-indexed).
fn backoff(attempt: u32) -> Duration {
    Duration::from_secs_f64(BACKOFF_BASE_SECONDS * f64::from(attempt))
}

/// Everything a cached HTTP fetch needs: the transport client, the blob
/// store, which trust prefix this fetch's namespace maps to, and whether
/// caching is enabled for this call. Bundled so fetcher call sites take one
/// parameter instead of four.
#[derive(Debug)]
pub(crate) struct FetchContext<'a> {
    client: &'a reqwest::Client,
    store: &'a storage::BlobStore,
    prefix: storage::StoragePrefix,
    cache_mode: CacheMode,
}

impl<'a> FetchContext<'a> {
    /// Bundle a fetch's transport, storage, trust prefix, and cache mode.
    pub(crate) fn new(
        client: &'a reqwest::Client,
        store: &'a storage::BlobStore,
        prefix: storage::StoragePrefix,
        cache_mode: CacheMode,
    ) -> Self {
        Self {
            client,
            store,
            prefix,
            cache_mode,
        }
    }
}

/// GET `url` as raw bytes, retrying with backoff, and consulting/populating
/// `ctx`'s blob store per `ctx.cache_mode`.
///
/// # Errors
///
/// Returns [`AcquireError::Http`] if every attempt fails, or
/// [`AcquireError::Cache`] if reading or writing the cache fails.
pub(crate) async fn get_bytes(ctx: &FetchContext<'_>, url: &str) -> Result<Vec<u8>, AcquireError> {
    if ctx.cache_mode == CacheMode::Enabled
        && let Some(cached) = ctx.store.read(ctx.prefix, url).await?
    {
        return Ok(cached);
    }

    let bytes = fetch_with_retry(ctx.client, url).await?;

    if ctx.cache_mode == CacheMode::Enabled {
        ctx.store.write(ctx.prefix, url, &bytes).await?;
    }

    Ok(bytes)
}

/// GET `url` and decode the response as JSON.
///
/// # Errors
///
/// Returns [`AcquireError::Http`]/[`AcquireError::Cache`] as [`get_bytes`], or
/// [`AcquireError::Json`] if the cached/fetched bytes do not parse as `T`.
pub(crate) async fn get_json<T: DeserializeOwned>(
    ctx: &FetchContext<'_>,
    url: &str,
) -> Result<T, AcquireError> {
    let bytes = get_bytes(ctx, url).await?;
    serde_json::from_slice(&bytes).map_err(|error| AcquireError::Json {
        url: url.to_owned(),
        source: error,
    })
}

/// GET `url` and return both the decoded text and the raw bytes it was
/// decoded from — callers checksum the *raw* bytes, matching the retired
/// Python prototype's behavior of hashing wire bytes rather than decoded text.
///
/// Decoding tries UTF-8 first; on failure it falls back to a lossy UTF-8
/// decode (replacing invalid sequences) rather than re-interpreting the bytes
/// as Latin-1, since most corpus sources (Gutenberg, datasets-server,
/// archive.org) declare or imply UTF-8 and a lossy decode is simpler to
/// reason about than silently misreading UTF-8 bytes as Latin-1.
///
/// # Errors
///
/// Returns [`AcquireError::Http`]/[`AcquireError::Cache`] as [`get_bytes`].
pub(crate) async fn get_text(
    ctx: &FetchContext<'_>,
    url: &str,
) -> Result<(String, Vec<u8>), AcquireError> {
    let bytes = get_bytes(ctx, url).await?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    Ok((text, bytes))
}

/// Build the shared HTTP client: polite UA, 30s timeout, transparent gzip
/// (via the `gzip` cargo feature).
///
/// # Errors
///
/// Returns [`AcquireError::Http`]-shaped construction failure surfaced as a
/// [`reqwest::Error`] wrapped by the caller; in practice this only fails on a
/// malformed TLS configuration, which cannot happen with the defaults used
/// here, but the fallible constructor is preserved rather than unwrapped.
pub(crate) fn build_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::ClientBuilder::new()
        .user_agent(USER_AGENT)
        .timeout(REQUEST_TIMEOUT)
        .build()
}

/// Why a single fetch attempt failed, carrying whether a retry could help.
enum AttemptError {
    /// Transport error, timeout, or a 5xx status — retrying may succeed.
    Transient(reqwest::Error),
    /// A 4xx client status — the request is wrong; retrying cannot help.
    Permanent(reqwest::Error),
    /// The response body crossed [`MAX_RESPONSE_BYTES`]; retrying cannot help.
    TooLarge,
}

/// Classify a `reqwest` error by whether retrying it could plausibly succeed: a
/// 4xx client status is permanent (the request is malformed or the resource is
/// gone), while a 5xx status, timeout, or transport failure is transient.
fn classify(error: reqwest::Error) -> AttemptError {
    match error.status() {
        Some(status) if status.is_client_error() => AttemptError::Permanent(error),
        _ => AttemptError::Transient(error),
    }
}

/// Perform the GET with up to [`MAX_ATTEMPTS`] tries and linear backoff between
/// them. Only transient failures are retried; a permanent (4xx) failure or an
/// over-cap body fails immediately rather than wasting further requests.
async fn fetch_with_retry(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, AcquireError> {
    let mut last_error = None;
    for attempt in 1..=MAX_ATTEMPTS {
        match attempt_once(client, url).await {
            Ok(bytes) => return Ok(bytes),
            Err(AttemptError::TooLarge) => {
                return Err(AcquireError::TooLarge {
                    url: url.to_owned(),
                    limit: MAX_RESPONSE_BYTES,
                });
            }
            Err(AttemptError::Permanent(error)) => {
                return Err(AcquireError::Http {
                    url: url.to_owned(),
                    attempts: attempt,
                    source: error,
                });
            }
            Err(AttemptError::Transient(error)) => {
                last_error = Some(error);
                if attempt < MAX_ATTEMPTS {
                    tokio::time::sleep(backoff(attempt)).await;
                }
            }
        }
    }
    Err(AcquireError::Http {
        url: url.to_owned(),
        attempts: MAX_ATTEMPTS,
        source: last_error.expect("loop runs at least once, so an error is always recorded"),
    })
}

/// A single GET attempt, streaming the response body in bounded chunks so the
/// accumulated (decompressed) size is checked against [`MAX_RESPONSE_BYTES`] as
/// it arrives rather than buffering an unbounded body up front.
async fn attempt_once(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, AttemptError> {
    let mut response = client
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(classify)?;

    // Fast reject when the server declares an over-cap length up front. This is ~keep
    // the compressed length (or absent under gzip), so the streaming check ~keep
    // below is the authoritative guard against decompression bombs. ~keep
    if response
        .content_length()
        .is_some_and(|len| len > MAX_RESPONSE_BYTES)
    {
        return Err(AttemptError::TooLarge);
    }

    let mut body: Vec<u8> = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(classify)? {
        if body.len() as u64 + chunk.len() as u64 > MAX_RESPONSE_BYTES {
            return Err(AttemptError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_prefixed_of_empty_bytes_matches_known_vector() {
        assert_eq!(
            sha256_prefixed(b""),
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn sha256_prefixed_of_known_text_matches_known_vector() {
        assert_eq!(
            sha256_prefixed(b"hello"),
            "sha256:2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn backoff_grows_linearly_with_attempt() {
        assert_eq!(backoff(1), Duration::from_secs_f64(1.5));
        assert_eq!(backoff(2), Duration::from_secs_f64(3.0));
        assert_eq!(backoff(3), Duration::from_secs_f64(4.5));
    }
}
