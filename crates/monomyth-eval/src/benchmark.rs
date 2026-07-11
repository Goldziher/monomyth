//! [`Benchmark`] — a pinned, content-hashed ground-truth fixture loader.
//!
//! ADR-0023 fixtures are full serialized `World`s under `artifacts/benchmarks/`,
//! authored by hand or via `apply_edits`, and — like the generator's golden-seed
//! output (`monomyth-gen/tests/determinism.rs`) — pinned with an FNV-1a hash so
//! an accidental edit to the fixture is caught the same way generator drift is.
//! `Benchmark::load` layers that pin on top of
//! [`World::from_json_checked`](monomyth_core::World::from_json_checked), so a
//! fixture is checked for schema version and referential integrity *and* for
//! being exactly the bytes it was reviewed as.

use monomyth_core::{LoadError, World};
use thiserror::Error;

use crate::util::fnv1a;

/// Why loading a benchmark fixture failed.
#[derive(Debug, Error)]
pub enum BenchmarkError {
    /// The raw JSON failed to parse into a [`World`], or the world's schema
    /// version did not match, or it failed [`World::validate`].
    #[error(transparent)]
    Load(#[from] LoadError),
    /// The fixture's content hash did not match the pinned expectation — the
    /// fixture bytes changed since the hash was last reviewed and re-pinned.
    #[error("fixture content hash {found:#018x} does not match pinned {expected:#018x}")]
    HashMismatch {
        /// The hash actually computed over the fixture's raw JSON bytes.
        found: u64,
        /// The hash the caller expected (from the benchmark manifest).
        expected: u64,
    },
}

/// A ground-truth fixture: a validated, schema-checked [`World`] whose raw JSON
/// bytes are pinned by an FNV-1a content hash.
#[derive(Debug, Clone)]
pub struct Benchmark {
    /// The loaded, validated gold world.
    pub world: World,
    /// The FNV-1a hash of the fixture's raw JSON bytes, for downstream
    /// provenance (e.g. recording which exact fixture version a `Report` was
    /// scored against).
    pub content_hash: u64,
}

impl Benchmark {
    /// Load a benchmark fixture from raw JSON bytes, requiring its content hash
    /// to match `expected_hash`.
    ///
    /// The hash is checked *before* parsing, so a fixture that was edited (even
    /// if the edit happens to still produce a valid `World`) is rejected up
    /// front rather than silently scored against unreviewed content — the same
    /// pin-then-parse discipline as `monomyth-gen`'s golden-seed test, applied to
    /// fixture content instead of generator output.
    ///
    /// # Errors
    ///
    /// Returns [`BenchmarkError::HashMismatch`] if the raw bytes do not hash to
    /// `expected_hash`, or [`BenchmarkError::Load`] if the (hash-verified) JSON
    /// fails to parse, fails the schema-version check, or fails
    /// [`World::validate`].
    pub fn load(json: &str, expected_hash: u64) -> Result<Self, BenchmarkError> {
        let found = fnv1a(json.as_bytes());
        if found != expected_hash {
            return Err(BenchmarkError::HashMismatch {
                found,
                expected: expected_hash,
            });
        }
        let world = World::from_json_checked(json)?;
        Ok(Self {
            world,
            content_hash: found,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monomyth_core::doc_support::single_room_world;

    #[test]
    fn load_should_accept_a_fixture_whose_hash_matches() {
        let json = serde_json::to_string(&single_room_world()).expect("world serializes");
        let expected_hash = fnv1a(json.as_bytes());

        let benchmark = Benchmark::load(&json, expected_hash).expect("hash matches, world valid");
        assert_eq!(benchmark.content_hash, expected_hash);
    }

    #[test]
    fn load_should_reject_a_fixture_whose_hash_does_not_match() {
        let json = serde_json::to_string(&single_room_world()).expect("world serializes");
        let wrong_hash = fnv1a(json.as_bytes()).wrapping_add(1);

        let error =
            Benchmark::load(&json, wrong_hash).expect_err("mismatched hash must be rejected");
        assert!(matches!(
            error,
            BenchmarkError::HashMismatch { expected, .. } if expected == wrong_hash
        ));
    }

    #[test]
    fn load_should_reject_invalid_json_even_when_the_hash_matches() {
        let json = "not valid json";
        let expected_hash = fnv1a(json.as_bytes());

        let error = Benchmark::load(json, expected_hash)
            .expect_err("hash matches but JSON does not parse as a World");
        assert!(matches!(
            error,
            BenchmarkError::Load(LoadError::Deserialize(_))
        ));
    }

    /// B2 supplies the real `artifacts/benchmarks/` fixture and its pinned hash
    /// from a manifest; until then, loading from a real file path is out of
    /// scope for this crate's B1 skeleton. This test documents the intended
    /// shape without depending on a fixture that does not exist yet.
    #[test]
    #[ignore = "B2 supplies the artifacts/benchmarks/ fixture and its pinned manifest hash"]
    fn load_should_read_a_real_benchmark_fixture_from_the_artifacts_directory() {
        unimplemented!("see artifacts/benchmarks/ once B2 lands");
    }
}
