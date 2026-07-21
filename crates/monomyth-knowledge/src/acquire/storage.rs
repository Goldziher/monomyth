//! The corpus blob store: a single `opendal::Operator` fronting the raw-fetch
//! cache and (later) the reference/inspect-only download area (ADR-0012).
//!
//! Amends ADR-0010's `tokio::fs`-only cache: the same read/write call sites
//! must work unchanged against a local filesystem in development and an
//! object-storage bucket in the cloud, selected by configuration rather than
//! a code fork. Only the filesystem service is wired this session — cloud
//! services are reserved, feature-gated sub-features on
//! [`monomyth-knowledge`]'s `Cargo.toml`, declared but not yet constructible
//! from here.

use std::path::Path;

use crate::acquire::error::AcquireError;
use crate::acquire::http::sha256_hex20;
use crate::ledger::Namespace;

/// Directory the on-disk fetch cache lives under, relative to the current
/// working directory — the same convention as the CLI's default `--db` path
/// (`./monomyth.db`).
pub(crate) const CACHE_DIR: &str = "corpus/raw";

/// Which trust domain a cached blob belongs to, expressed as the path prefix
/// an [`opendal::Operator`] key is written under.
///
/// This is the ADR-0005 ship/reference licensing invariant enforced at the
/// storage layer: a namespace that isn't [`Namespace::Ship`] always lands
/// under the `reference/` prefix, never `ship/`, on every backend the
/// operator is configured against (local FS today; a bucket service later).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoragePrefix {
    /// Verified ship-safe material — may be surfaced verbatim.
    Ship,
    /// Reference/unverified material — informs generation only, never
    /// ingested into the ship collection.
    Reference,
    /// User-uploaded material — non-surfaceable by default, never ingested
    /// into the ship collection (ADR-0019).
    User,
}

impl StoragePrefix {
    /// The path segment this prefix maps to within the operator.
    pub(crate) const fn segment(self) -> &'static str {
        match self {
            Self::Ship => "ship",
            Self::Reference => "reference",
            Self::User => "user",
        }
    }
}

impl From<Namespace> for StoragePrefix {
    /// `Namespace::Ship` maps to `Self::Ship`, `Namespace::Reference` to
    /// `Self::Reference`, `Namespace::User` to `Self::User`. The match is
    /// exhaustive over `Namespace`: adding a further variant will not compile
    /// until this mapping is extended, forcing a deliberate trust-domain
    /// decision at the type level rather than letting a new namespace silently
    /// fall through to a default prefix.
    fn from(namespace: Namespace) -> Self {
        match namespace {
            Namespace::Ship => Self::Ship,
            Namespace::Reference => Self::Reference,
            Namespace::User => Self::User,
        }
    }
}

/// The corpus blob store: one [`opendal::Operator`] plus the key scheme that
/// keeps ship and reference material physically separated on whatever
/// backend the operator is configured against.
#[derive(Clone)]
pub(crate) struct BlobStore {
    operator: opendal::Operator,
}

impl std::fmt::Debug for BlobStore {
    /// [`opendal::Operator`] does not implement [`std::fmt::Debug`], so this
    /// prints a fixed marker rather than the operator's internals.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("BlobStore").finish_non_exhaustive()
    }
}

impl BlobStore {
    /// Build a filesystem-backed store rooted at `root`.
    ///
    /// # Errors
    ///
    /// Returns [`AcquireError::Cache`] if the operator cannot be constructed
    /// (e.g. `root` cannot be created or canonicalized).
    pub(crate) fn at_root(root: &Path) -> Result<Self, AcquireError> {
        let root_display = root.display().to_string();
        let operator = opendal::Operator::new(opendal::services::Fs::default().root(&root_display))
            .map_err(|source| AcquireError::Cache {
                path: root_display,
                source,
            })?
            .finish();
        Ok(Self { operator })
    }

    /// Build the store used by [`crate::acquire::build_corpus`]: filesystem,
    /// rooted at [`CACHE_DIR`].
    ///
    /// # Errors
    ///
    /// Returns [`AcquireError::Cache`] as [`Self::at_root`].
    pub(crate) fn local() -> Result<Self, AcquireError> {
        Self::at_root(Path::new(CACHE_DIR))
    }

    /// Read the cached blob for `url` under `prefix`, if present.
    ///
    /// # Errors
    ///
    /// Returns [`AcquireError::Cache`] on any operator failure other than a
    /// missing key.
    pub(crate) async fn read(
        &self,
        prefix: StoragePrefix,
        url: &str,
    ) -> Result<Option<Vec<u8>>, AcquireError> {
        let key = cache_key(prefix, url);
        match self.operator.read(&key).await {
            Ok(buffer) => Ok(Some(buffer.to_vec())),
            Err(error) if error.kind() == opendal::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(AcquireError::Cache {
                path: key,
                source: error,
            }),
        }
    }

    /// Write `bytes` as the cached blob for `url` under `prefix`. The
    /// filesystem service creates parent directories automatically, so no
    /// separate mkdir step is needed.
    ///
    /// # Errors
    ///
    /// Returns [`AcquireError::Cache`] if the write fails.
    pub(crate) async fn write(
        &self,
        prefix: StoragePrefix,
        url: &str,
        bytes: &[u8],
    ) -> Result<(), AcquireError> {
        let key = cache_key(prefix, url);
        self.operator
            .write(&key, bytes.to_vec())
            .await
            .map(|_metadata| ())
            .map_err(|error| AcquireError::Cache {
                path: key,
                source: error,
            })
    }
}

/// The operator key for `url` under `prefix`:
/// `"<segment>/<sha256(url)[:20 hex]>.bin"`. The hash truncation is shared
/// with [`crate::acquire::http::sha256_prefixed`]'s checksum logic via
/// [`sha256_hex20`], so cache keys stay byte-for-byte stable across the
/// ADR-0012 rewire.
fn cache_key(prefix: StoragePrefix, url: &str) -> String {
    format!("{}/{}.bin", prefix.segment(), sha256_hex20(url.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store(root: &Path) -> BlobStore {
        BlobStore::at_root(root).expect("fs operator builds against a tempdir")
    }

    #[tokio::test]
    async fn write_then_read_round_trips_the_exact_bytes() {
        let dir = tempfile::tempdir().expect("tempdir creates");
        let store = store(dir.path());
        let url = "https://example.org/a.txt";

        store
            .write(StoragePrefix::Ship, url, b"hello world")
            .await
            .expect("write succeeds");
        let read_back = store
            .read(StoragePrefix::Ship, url)
            .await
            .expect("read succeeds");

        assert_eq!(read_back, Some(b"hello world".to_vec()));
    }

    #[tokio::test]
    async fn read_of_an_unwritten_url_returns_none() {
        let dir = tempfile::tempdir().expect("tempdir creates");
        let store = store(dir.path());

        let read_back = store
            .read(
                StoragePrefix::Reference,
                "https://example.org/never-written.txt",
            )
            .await
            .expect("read succeeds");

        assert_eq!(read_back, None);
    }

    #[tokio::test]
    async fn the_same_url_under_ship_and_reference_lands_under_distinct_keys() {
        let dir = tempfile::tempdir().expect("tempdir creates");
        let store = store(dir.path());
        let url = "https://example.org/shared.txt";

        store
            .write(StoragePrefix::Ship, url, b"ship bytes")
            .await
            .expect("ship write succeeds");
        store
            .write(StoragePrefix::Reference, url, b"reference bytes")
            .await
            .expect("reference write succeeds");

        let ship = store
            .read(StoragePrefix::Ship, url)
            .await
            .expect("ship read succeeds");
        let reference = store
            .read(StoragePrefix::Reference, url)
            .await
            .expect("reference read succeeds");

        assert_eq!(ship, Some(b"ship bytes".to_vec()));
        assert_eq!(reference, Some(b"reference bytes".to_vec()));

        let ship_key = cache_key(StoragePrefix::Ship, url);
        let reference_key = cache_key(StoragePrefix::Reference, url);
        assert_ne!(ship_key, reference_key);
        assert!(ship_key.starts_with("ship/"));
        assert!(reference_key.starts_with("reference/"));
    }

    #[test]
    fn cache_key_is_deterministic_and_hash_derived() {
        let first = cache_key(StoragePrefix::Ship, "https://example.org/a.txt");
        let second = cache_key(StoragePrefix::Ship, "https://example.org/a.txt");
        assert_eq!(
            first, second,
            "the same (prefix, url) must hash to the same key"
        );

        let different_url = cache_key(StoragePrefix::Ship, "https://example.org/b.txt");
        assert_ne!(first, different_url, "different URLs must hash differently");

        let different_prefix = cache_key(StoragePrefix::Reference, "https://example.org/a.txt");
        assert_ne!(
            first, different_prefix,
            "the same URL under a different prefix must hash differently"
        );
        assert!(different_prefix.starts_with("reference/"));

        let hash_segment = first
            .strip_prefix("ship/")
            .and_then(|rest| rest.strip_suffix(".bin"))
            .expect("key has the expected \"ship/<hash>.bin\" shape");
        assert_eq!(
            hash_segment.len(),
            20,
            "the hash segment must be exactly 20 hex chars"
        );
    }
}
