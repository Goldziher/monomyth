//! Determinism guarantees for [`Generator::generate_structure`].
//!
//! The procedural half of generation is a pure function of its seed. These tests
//! prove reproducibility (byte-identical serialized output), seed sensitivity
//! (different seeds diverge), and stability (a pinned golden hash catches
//! unintended output drift).

use monomyth_gen::{GenerationConfig, Generator};

/// The seed the golden hash is pinned against.
const GOLDEN_SEED: u64 = 42;

/// The fork probability (permille) the golden hash was pinned against — the
/// `with_default_passes` / `NarrativeConfig::default` value. Fed explicitly through
/// [`Generator::with_config`] below to pin the config-threading plumbing to the
/// golden, independently of `GenerationConfig::default`.
const GOLDEN_FORK_CHANCE_PERMILLE: u16 = 500;

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// The pinned FNV-1a hash of the seed-42 serialized world.
///
/// An intentional change to the generator (new pass, changed layout, reordered
/// draws) is expected to change this value; update it deliberately and review the
/// diff. An unexpected change means the generator lost determinism or drifted.
///
/// Deliberately re-pinned for ADR-0022 Phase S1: `NarrativeNode.functions` moved
/// from an unweighted `BTreeSet<ProppFunction>` to the scored
/// `ScoredSet<ProppFunction>` shape, so both the serialized form (a permille
/// weighted object, e.g. `{"VillainyOrLack":1000,"Mediation":1000}`, instead of a
/// bare JSON array) and the draw modulus (`draw_weighted_index`'s cumulative-sum
/// reduction instead of `draw_indexed_subset`'s uniform swap-remove) changed, per
/// the ADR's Confirmation section — a byte-identical golden is not expected to
/// survive this change.
///
/// Deliberately re-pinned again for ADR-0022 Phase S2: the remaining five
/// framework-typed fields (`NarrativeNode.motifs`/`.situation`/`.stage`,
/// `Story.plot`, `Quest.situation`, `Entity.role`/`.archetype`) moved from bare or
/// unweighted shapes to `ScoredOne`/`ScoredSet`, moving the golden hash from
/// `0x62a2_24a2_27e0_61a8` to `0x65f6_6a4b_a541_5419`, so both the serialized
/// shape (mandatory `stage` now serializes as a `{"primary":...,"alternatives":
/// {...}}` object instead of a bare string, etc.) and the draw sequence (motif
/// selection now goes through `draw_weighted_index` instead of the uniform
/// `draw_indexed_subset`) changed, per the ADR's Confirmation section.
const GOLDEN_SEED_42_FNV1A: u64 = 0x65f6_6a4b_a541_5419;

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

#[test]
fn config_default_path_matches_default_passes() {
    // `with_default_passes` delegates to `with_config(&GenerationConfig::default())`,
    // so the resolved-default config path must reproduce the golden exactly. This
    // is the anti-drift ratchet: no config-threading change may alter an
    // unconfigured run's output.
    let world = Generator::with_config(&GenerationConfig::default())
        .generate_structure(GOLDEN_SEED)
        .expect("the default-config pipeline generates a world");
    let hash = fnv1a(
        serde_json::to_string(&world)
            .expect("serializes")
            .as_bytes(),
    );
    assert_eq!(
        hash, GOLDEN_SEED_42_FNV1A,
        "the GenerationConfig::default() path drifted from the golden",
    );
}

#[test]
fn explicit_golden_fork_chance_reproduces_the_golden() {
    // Pin the semantic binding "fork_chance 500 -> golden" with an explicitly
    // constructed config (not `::default()`), so a change to the shipped default
    // fork probability is caught here as well as in `golden_hash_is_stable`.
    let config = GenerationConfig {
        fork_chance_permille: GOLDEN_FORK_CHANCE_PERMILLE,
        ..GenerationConfig::default()
    };
    let world = Generator::with_config(&config)
        .generate_structure(GOLDEN_SEED)
        .expect("the explicit-config pipeline generates a world");
    let hash = fnv1a(
        serde_json::to_string(&world)
            .expect("serializes")
            .as_bytes(),
    );
    assert_eq!(
        hash, GOLDEN_SEED_42_FNV1A,
        "fork_chance {GOLDEN_FORK_CHANCE_PERMILLE} no longer reproduces the golden",
    );
}
