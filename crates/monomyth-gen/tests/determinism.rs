//! Determinism guarantees for [`Generator::generate_structure`].
//!
//! The procedural half of generation is a pure function of its seed. These tests
//! prove reproducibility (byte-identical serialized output), seed sensitivity
//! (different seeds diverge), and stability (a pinned golden hash catches
//! unintended output drift).

use monomyth_gen::Generator;

/// The seed the golden hash is pinned against.
const GOLDEN_SEED: u64 = 42;

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// The pinned FNV-1a hash of the seed-42 serialized world.
///
/// An intentional change to the generator (new pass, changed layout, reordered
/// draws) is expected to change this value; update it deliberately and review the
/// diff. An unexpected change means the generator lost determinism or drifted.
const GOLDEN_SEED_42_FNV1A: u64 = 0x9780_5926_969d_4982;

/// A tiny, dependency-free FNV-1a over raw bytes.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Serialize a freshly generated world for `seed` to a JSON string.
fn serialized(seed: u64) -> String {
    let world = Generator::with_default_passes()
        .generate_structure(seed)
        .expect("the default pipeline generates a world");
    serde_json::to_string(&world).expect("a world serializes to JSON")
}

#[test]
fn same_seed_is_byte_identical() {
    assert_eq!(
        serialized(GOLDEN_SEED),
        serialized(GOLDEN_SEED),
        "the same seed must reproduce byte-identical output",
    );
}

#[test]
fn different_seeds_diverge() {
    assert_ne!(
        serialized(42),
        serialized(43),
        "different seeds must drive different output",
    );
}

#[test]
fn golden_hash_is_stable() {
    let hash = fnv1a(serialized(GOLDEN_SEED).as_bytes());
    assert_eq!(
        hash, GOLDEN_SEED_42_FNV1A,
        "generator output drifted; if intentional, update GOLDEN_SEED_42_FNV1A to {hash:#018x}",
    );
}
