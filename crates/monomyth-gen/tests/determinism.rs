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

// The remaining knobs below round out ADR-0015's Confirmation requirement: "a test
// resolves a changed config value and asserts downstream RNG sub-streams outside the
// owning pass are byte-identical (only the intended sub-stream's draws differ)". The
// procedural pipeline runs `BackbonePass -> BeatPass -> MapPass -> CastPass ->
// ItemsPass`, and each pass draws its own child seed from a root RNG seeded once from
// the world seed (see `Generator::generate_structure`); child seeds are handed out in
// pipeline order regardless of how many words an earlier pass consumes, so changing
// one pass's *own* config knob can never shift another pass's sub-stream. That lets
// each test below assert byte-identical output on the serialized facet(s) owned by
// every *other* pass.

/// A world generated from `seed` under `config`, for building override configs
/// in the tests below.
fn generated_world(seed: u64, config: &GenerationConfig) -> monomyth_core::World {
    Generator::with_config(config)
        .generate_structure(seed)
        .expect("the configured pipeline generates a world")
}

/// A world generated from `seed` under the default config, serialized to JSON.
fn default_serialized(seed: u64) -> String {
    serde_json::to_string(&generated_world(seed, &GenerationConfig::default()))
        .expect("a world serializes to JSON")
}

/// `beats_per_stage_min`/`beats_per_stage_max`, forced equal and above the default
/// `1..=3` band, so every stage node splices exactly this many beats — guaranteed to
/// change the narrative structure's node count relative to the default config.
const BEATS_PER_STAGE_FIXED: usize = 5;

#[test]
fn beat_config_changes_structure_deterministically_and_leaves_world_content_untouched() {
    let config = GenerationConfig {
        beats_per_stage_min: BEATS_PER_STAGE_FIXED,
        beats_per_stage_max: BEATS_PER_STAGE_FIXED,
        ..GenerationConfig::default()
    };

    // Effect: forcing a fixed beat count outside the default 1..=3 band must change
    // the serialized world relative to the default config.
    let changed = serde_json::to_string(&generated_world(GOLDEN_SEED, &config))
        .expect("a world serializes to JSON");
    assert_ne!(
        changed,
        default_serialized(GOLDEN_SEED),
        "beats_per_stage_min/max must be wired into BeatPass's draws",
    );

    // Determinism: the same (seed, config) reproduces byte-identical output.
    let changed_again = serde_json::to_string(&generated_world(GOLDEN_SEED, &config))
        .expect("a world serializes to JSON");
    assert_eq!(
        changed, changed_again,
        "the same seed and beat config must reproduce byte-identical output",
    );

    // Isolation: BeatPass runs after BackbonePass and before MapPass/CastPass/
    // ItemsPass. It only edits `world.story.structure` (via the narrative edit
    // surface), so the location graph, cast, and item placements — all drawn from
    // sub-streams handed out independently of BeatPass's own draw count — must stay
    // byte-identical to the default-config run.
    let default_world = generated_world(GOLDEN_SEED, &GenerationConfig::default());
    let changed_world = generated_world(GOLDEN_SEED, &config);
    assert_eq!(
        serde_json::to_string(&changed_world.locations).expect("serializes"),
        serde_json::to_string(&default_world.locations).expect("serializes"),
        "beats_per_stage_min/max must not perturb the location graph",
    );
    assert_eq!(
        serde_json::to_string(&changed_world.entities).expect("serializes"),
        serde_json::to_string(&default_world.entities).expect("serializes"),
        "beats_per_stage_min/max must not perturb the cast",
    );
    assert_eq!(
        serde_json::to_string(&changed_world.items).expect("serializes"),
        serde_json::to_string(&default_world.items).expect("serializes"),
        "beats_per_stage_min/max must not perturb item placement",
    );
}

/// `rooms_min`/`rooms_max`, forced equal and above the default `5..=9` band, so the
/// map always contains exactly this many rooms — guaranteed to change the location
/// graph relative to the default config.
const ROOMS_FIXED: usize = 12;

#[test]
fn map_config_changes_locations_deterministically_and_leaves_the_story_spine_untouched() {
    let config = GenerationConfig {
        rooms_min: ROOMS_FIXED,
        rooms_max: ROOMS_FIXED,
        ..GenerationConfig::default()
    };

    // Effect: forcing a fixed room count outside the default 5..=9 band must change
    // the serialized world relative to the default config.
    let changed = serde_json::to_string(&generated_world(GOLDEN_SEED, &config))
        .expect("a world serializes to JSON");
    assert_ne!(
        changed,
        default_serialized(GOLDEN_SEED),
        "rooms_min/rooms_max must be wired into MapPass's draws",
    );

    // Determinism: the same (seed, config) reproduces byte-identical output.
    let changed_again = serde_json::to_string(&generated_world(GOLDEN_SEED, &config))
        .expect("a world serializes to JSON");
    assert_eq!(
        changed, changed_again,
        "the same seed and map config must reproduce byte-identical output",
    );

    // Isolation: MapPass runs after BackbonePass and BeatPass and only writes
    // `world.locations` plus `world.player.location`. Neither earlier pass reads
    // MapPass's config or output, so `world.story` (the narrative structure, plot,
    // and quests BackbonePass/BeatPass built) must stay byte-identical to the
    // default-config run. (CastPass/ItemsPass run after MapPass and *do* read the
    // resulting room count, so `world.entities`/`world.items` are expected to
    // differ here and are intentionally not asserted.)
    let default_world = generated_world(GOLDEN_SEED, &GenerationConfig::default());
    let changed_world = generated_world(GOLDEN_SEED, &config);
    assert_eq!(
        serde_json::to_string(&changed_world.story).expect("serializes"),
        serde_json::to_string(&default_world.story).expect("serializes"),
        "rooms_min/rooms_max must not perturb the story spine",
    );
}

/// `items_min`/`items_max`, forced equal and above the default `2..=6` band, so
/// exactly this many items are scattered — guaranteed to change item placement
/// relative to the default config.
const ITEMS_FIXED: usize = 10;

#[test]
fn items_config_changes_items_deterministically_and_leaves_upstream_facets_untouched() {
    let config = GenerationConfig {
        items_min: ITEMS_FIXED,
        items_max: ITEMS_FIXED,
        ..GenerationConfig::default()
    };

    // Effect: forcing a fixed item count outside the default 2..=6 band must change
    // the serialized world relative to the default config.
    let changed = serde_json::to_string(&generated_world(GOLDEN_SEED, &config))
        .expect("a world serializes to JSON");
    assert_ne!(
        changed,
        default_serialized(GOLDEN_SEED),
        "items_min/items_max must be wired into ItemsPass's draws",
    );

    // Determinism: the same (seed, config) reproduces byte-identical output.
    let changed_again = serde_json::to_string(&generated_world(GOLDEN_SEED, &config))
        .expect("a world serializes to JSON");
    assert_eq!(
        changed, changed_again,
        "the same seed and items config must reproduce byte-identical output",
    );

    // Isolation: ItemsPass is last in the pipeline (Backbone -> Beat -> Map -> Cast
    // -> Items) and only writes `world.items` (plus each item id into its room's
    // `Location::items`, folded into `world.locations` below). Nothing downstream
    // reads its output, so the story spine, the location graph's own layout, and the
    // cast — all built by earlier passes with independently seeded sub-streams —
    // must stay byte-identical to the default-config run.
    let default_world = generated_world(GOLDEN_SEED, &GenerationConfig::default());
    let changed_world = generated_world(GOLDEN_SEED, &config);
    assert_eq!(
        serde_json::to_string(&changed_world.story).expect("serializes"),
        serde_json::to_string(&default_world.story).expect("serializes"),
        "items_min/items_max must not perturb the story spine",
    );
    assert_eq!(
        serde_json::to_string(&changed_world.entities).expect("serializes"),
        serde_json::to_string(&default_world.entities).expect("serializes"),
        "items_min/items_max must not perturb the cast",
    );
}

/// `max_extra_cast`, forced well above the default `3`. `CastPass` draws the number
/// of supporting roles with `draw_range_inclusive(rng, 0, max_extra_cast)`, so this
/// value is the draw's inclusive upper bound: widening it from 3 to 20 changes the
/// draw's range and therefore the drawn count for seed 42, changing the cast — even
/// though the 5-entry role pool caps how many roles are ultimately realized.
const MAX_EXTRA_CAST_FIXED: usize = 20;

#[test]
fn cast_config_changes_cast_deterministically_and_leaves_the_map_and_spine_untouched() {
    let config = GenerationConfig {
        max_extra_cast: MAX_EXTRA_CAST_FIXED,
        ..GenerationConfig::default()
    };

    // Effect: widening the extra-cast draw ceiling well past the default of 3 must
    // change the serialized world relative to the default config.
    let changed = serde_json::to_string(&generated_world(GOLDEN_SEED, &config))
        .expect("a world serializes to JSON");
    assert_ne!(
        changed,
        default_serialized(GOLDEN_SEED),
        "max_extra_cast must be wired into CastPass's draws",
    );

    // Determinism: the same (seed, config) reproduces byte-identical output.
    let changed_again = serde_json::to_string(&generated_world(GOLDEN_SEED, &config))
        .expect("a world serializes to JSON");
    assert_eq!(
        changed, changed_again,
        "the same seed and cast config must reproduce byte-identical output",
    );

    // Isolation: CastPass runs after BackbonePass/BeatPass/MapPass and writes
    // `world.entities` plus each new entity's id into its room's
    // `Location::entities` set — so `world.locations` itself is *not* a clean
    // isolation claim (CastPass legitimately mutates it) and is deliberately not
    // asserted here. What CastPass never touches is the story spine: it neither
    // reads nor edits `world.story`, which BackbonePass/BeatPass already finished
    // building, so that facet must stay byte-identical to the default-config run.
    // (ItemsPass runs after CastPass and does not read the cast, so `world.items`
    // is expected to be unaffected too, but is out of scope for this pass's
    // isolation claim and left unasserted.)
    let default_world = generated_world(GOLDEN_SEED, &GenerationConfig::default());
    let changed_world = generated_world(GOLDEN_SEED, &config);
    assert_eq!(
        serde_json::to_string(&changed_world.story).expect("serializes"),
        serde_json::to_string(&default_world.story).expect("serializes"),
        "max_extra_cast must not perturb the story spine",
    );
}
