//! Cached, retrying HTTP GET for corpus acquisition.
//!
//! Ports the retired Python prototype's `http.py`: a polite `User-Agent`, an
//! on-disk raw-bytes cache keyed by a hash of the URL (so re-runs are
//! idempotent and offline-capable), retry with backoff, and a 30s timeout.
//! Gzip is handled transparently by reqwest's `gzip` feature rather than
//! hand-rolled decompression.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};

use crate::acquire::error::AcquireError;

/// Identifies this pipeline to remote servers as a polite, identifiable client.
const USER_AGENT: &str = "monomyth-corpus/0.1 (+https://github.com/monomyth; research/indexing)";

/// Directory the on-disk fetch cache lives under, relative to the current
/// working directory — the same convention as the CLI's default `--db` path
/// (`./monomyth.db`).
const CACHE_DIR: &str = "corpus/raw";

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
fn write_hex(out: &mut String, bytes: &[u8]) {
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

/// The on-disk cache path for `url`: `corpus/raw/<sha256(url)[:20]>.bin`.
fn cache_path(url: &str) -> PathBuf {
    let mut hasher = Sha256::new();
    hasher.update(url.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    write_hex(&mut hex, &digest);
    hex.truncate(20);
    Path::new(CACHE_DIR).join(format!("{hex}.bin"))
}

/// Backoff duration before the attempt after `attempt` (1-indexed).
fn backoff(attempt: u32) -> Duration {
    Duration::from_secs_f64(BACKOFF_BASE_SECONDS * f64::from(attempt))
}

/// GET `url` as raw bytes, retrying with backoff, and consulting/populating
/// the on-disk cache per `cache_mode`.
///
/// # Errors
///
/// Returns [`AcquireError::Http`] if every attempt fails, or
/// [`AcquireError::Cache`] if reading or writing the cache fails.
pub(crate) async fn get_bytes(
    client: &reqwest::Client,
    url: &str,
    cache_mode: CacheMode,
) -> Result<Vec<u8>, AcquireError> {
    let path = cache_path(url);

    if cache_mode == CacheMode::Enabled
        && let Some(cached) = read_cache(&path).await?
    {
        return Ok(cached);
    }

    let bytes = fetch_with_retry(client, url).await?;

    if cache_mode == CacheMode::Enabled {
        write_cache(&path, &bytes).await?;
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
    client: &reqwest::Client,
    url: &str,
    cache_mode: CacheMode,
) -> Result<T, AcquireError> {
    let bytes = get_bytes(client, url, cache_mode).await?;
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
    client: &reqwest::Client,
    url: &str,
    cache_mode: CacheMode,
) -> Result<(String, Vec<u8>), AcquireError> {
    let bytes = get_bytes(client, url, cache_mode).await?;
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

    // Fast reject when the server declares an over-cap length up front. This is
    // the compressed length (or absent under gzip), so the streaming check
    // below is the authoritative guard against decompression bombs.
    if response.content_length().is_some_and(|len| len > MAX_RESPONSE_BYTES) {
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

/// Read `path` from the on-disk cache, if present.
async fn read_cache(path: &Path) -> Result<Option<Vec<u8>>, AcquireError> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AcquireError::Cache {
            path: path.display().to_string(),
            source: error,
        }),
    }
}

/// Write `bytes` to `path`, creating the parent cache directory if needed.
async fn write_cache(path: &Path, bytes: &[u8]) -> Result<(), AcquireError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| AcquireError::Cache {
                path: parent.display().to_string(),
                source: error,
            })?;
    }
    tokio::fs::write(path, bytes)
        .await
        .map_err(|error| AcquireError::Cache {
            path: path.display().to_string(),
            source: error,
        })
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
    fn cache_path_is_deterministic_and_hash_derived() {
        let first = cache_path("https://example.org/a.txt");
        let second = cache_path("https://example.org/a.txt");
        assert_eq!(first, second, "the same URL must hash to the same path");

        let different = cache_path("https://example.org/b.txt");
        assert_ne!(first, different, "different URLs must hash differently");

        assert_eq!(
            first.extension().and_then(|extension| extension.to_str()),
            Some("bin")
        );
        let file_name = first
            .file_name()
            .and_then(|name| name.to_str())
            .expect("cache path has a UTF-8 file name");
        assert_eq!(
            file_name.len(),
            "00000000000000000000.bin".len(),
            "the hash prefix must be exactly 20 hex chars"
        );
    }

    #[test]
    fn backoff_grows_linearly_with_attempt() {
        assert_eq!(backoff(1), Duration::from_secs_f64(1.5));
        assert_eq!(backoff(2), Duration::from_secs_f64(3.0));
        assert_eq!(backoff(3), Duration::from_secs_f64(4.5));
    }
}
