//! The Odyssey Campbell-macro-axis ground-truth fixture (ADR-0023, Phase B2).
//!
//! Builds `artifacts/benchmarks/greek/odyssey.json` from the reviewable edit-script
//! `artifacts/benchmarks/greek/odyssey.edits.json`, and pins both against the
//! committed bytes and the FNV hash recorded in `artifacts/benchmarks/index.json` —
//! the same drift-detecting discipline `monomyth-gen`'s golden-seed test applies to
//! generator output, applied here to a hand-encoded ground-truth fixture instead.
//!
//! The stage → episode annotations are drawn from the banked ground-truth research
//! (fabula-order Campbell-macro reading of Homer's *Odyssey*, Samuel Butler 1900
//! translation, Project Gutenberg #1727 — `source_id = "gutenberg_odyssey_butler"`
//! in `corpus/manifest.json`). Every `Content` slot stays `Content::Empty`: no
//! prose, PD or otherwise, is ever baked into a fixture.

use std::collections::BTreeSet;

use monomyth_core::{
    Content, ContentKind, ContentPrompt, EdgeKind, EditOutcome, Entity, EntityKind, Location,
    NarrativeEdit, NodeSpec, Player, RngState, SCHEMA_VERSION, ScoredOne, Story, Weight, World,
    WorldMeta, WorldState,
};
use monomyth_frameworks::{MonomythStage, ProppRole};
use monomyth_knowledge::Ledger;
use slotmap::SlotMap;

/// The ledger `source_id` this fixture is grounded in (see `corpus/manifest.json`).
const ODYSSEY_SOURCE_ID: &str = "gutenberg_odyssey_butler";

/// The pinned FNV-1a hash of `artifacts/benchmarks/greek/odyssey.json`'s exact
/// committed bytes, mirrored in `artifacts/benchmarks/index.json`. Re-pin both
/// together (as an eyeballed diff, per ADR-0014) if the fixture is intentionally
/// revised.
const ODYSSEY_CONTENT_HASH: u64 = 0x60d0_92be_d277_40f9;

fn synopsis(hint: &str) -> Content {
    Content::empty(ContentPrompt::new(ContentKind::Synopsis, hint))
}

/// One node's authoring recipe: its structural label, grounding hint, and scored
/// stage (primary plus zero-or-more weighted alternatives).
struct StageNode {
    label: &'static str,
    hint: &'static str,
    primary: MonomythStage,
    /// `(alternative stage, permille weight)` — the scholarly-wobble split.
    alternatives: &'static [(MonomythStage, u16)],
}

/// The Odyssey macro spine in fabula (story) order, not text (poem) order — the
/// poem opens in medias res at year 10 and narrates Books 9-12 as Odysseus's own
/// flashback at the Phaeacian court (research note: non-linearity flag #2). The
/// fixture encodes the underlying event order, which is what a Campbell-macro
/// extractor is ultimately scored against.
///
/// Confidence and permille splits (see the banked research,
/// `odyssey-ground-truth.md`):
///
/// - `CallToAdventure`: medium confidence (Telemachus arc; the Troy summons itself
///   is off-page) — primary only, no competing stage reading.
/// - `SupernaturalAid`: HIGH (Athena's patronage runs through the whole epic) —
///   primary only.
/// - `CrossingTheFirstThreshold`: medium, ambiguous *which* departure counts
///   (Troy vs. Lotus-Eaters vs. Cyclops island) — but the ambiguity is over which
///   episode instantiates the stage, not over competing *stages*, so it stays
///   primary-only; the episode-identification ambiguity is out of scope for a
///   macro-axis (stage-only) fixture.
/// - `BellyOfTheWhale`: HIGH (Polyphemus's cave: enclosure, "Nobody" trick, escape
///   under the ram) — primary only.
/// - `TheRoadOfTrials`: HIGH, the spine of Books 9-12 — primary only.
/// - The Circe/Calypso split: the single most legitimate scholarly ambiguity
///   (research note #5) is encoded as a genuine `ScoredOne` split across two
///   *stages*, not just two figures — `TheMeetingWithTheGoddess` (Circe, primary,
///   650‰) with `WomanAsTemptress` (Calypso, alternative, 350‰). The 650/350 split
///   reflects that Circe is the more textually dominant "unconditional aid" figure
///   (she frees the crew, gives passage instructions) while Calypso's temptation
///   (the immortality offer, 7 years) is real but the poem spends comparatively
///   less time dramatizing it as temptation versus Circe's aid.
/// - `TheUltimateBoon`: medium (Tiresias's prophecy is knowledge, not an object;
///   the research's noted alt candidate — "Phaeacian treasure/homecoming" — is not
///   itself a distinct Campbell stage, so there is no second *stage* to score as
///   an alternative here) — primary only.
/// - `RefusalOfTheReturn`: medium (lingering with Calypso 7y, Circe 1y) — primary
///   only; no competing stage reading, just an intensity judgment already implicit
///   in choosing to realize the stage at all.
/// - `TheMagicFlight`: HIGH (the Phaeacians' magic ship) — primary only.
/// - `RescueFromWithout`: HIGH (Phaeacians + Athena; Ino/Leucothea's storm rescue)
///   — primary only.
/// - `TheCrossingOfTheReturnThreshold`: HIGH (disguised-beggar landing on Ithaca)
///   — primary only.
/// - `MasterOfTheTwoWorlds`: HIGH (suitor-slaughter, Athena-sanctioned kingship)
///   — primary only.
/// - `FreedomToLive`: HIGH (Penelope bed-recognition, Laertes, Athena-brokered
///   peace, Books 23-24) — primary only.
///
/// Explicitly omitted (per the research's "explicitly poor/unrealized" list):
/// `RefusalOfTheCall` (feigned-madness draft-dodge is extra-Homeric — absent from
/// the text, not merely weak), `AtonementWithTheFather` (no true father-atonement;
/// Anticleia/Tiresias and the Laertes reunion don't instantiate this stage), and
/// `Apotheosis` (the Nekyia is a contested, not a clean, reading — Odysseus is
/// mortal and never deifies). Omitting rather than low-scoring these keeps every
/// realized node an honest primary reading instead of diluting `ScoredOne`'s
/// mandatory-primary semantics with a manufactured low-confidence label; a future
/// extractor is expected to *not* propose these three for the Odyssey, and the
/// eval harness scores that as a true negative once axis-level recall is wired up.
fn odyssey_spine() -> Vec<StageNode> {
    vec![
        StageNode {
            label: "CallToAdventure",
            hint: "Telemachus, spurred by Athena, sets out to learn his father's fate",
            primary: MonomythStage::CallToAdventure,
            alternatives: &[],
        },
        StageNode {
            label: "SupernaturalAid",
            hint: "Athena's patronage and counsel across the journey",
            primary: MonomythStage::SupernaturalAid,
            alternatives: &[],
        },
        StageNode {
            label: "CrossingTheFirstThreshold",
            hint: "departure from the known world toward the Cyclopes' island",
            primary: MonomythStage::CrossingTheFirstThreshold,
            alternatives: &[],
        },
        StageNode {
            label: "BellyOfTheWhale",
            hint: "the Cyclops's cave: enclosure, the Nobody trick, escape under the ram",
            primary: MonomythStage::BellyOfTheWhale,
            alternatives: &[],
        },
        StageNode {
            label: "TheRoadOfTrials",
            hint: "the serial ordeals: Aeolus's winds, the Laestrygonians, Scylla and Charybdis",
            primary: MonomythStage::TheRoadOfTrials,
            alternatives: &[],
        },
        StageNode {
            label: "GoddessAndTemptress",
            hint: "Circe's aid and transformation of the crew, and Calypso's offer of immortality",
            primary: MonomythStage::TheMeetingWithTheGoddess,
            alternatives: &[(MonomythStage::WomanAsTemptress, 350)],
        },
        StageNode {
            label: "TheUltimateBoon",
            hint: "Tiresias's prophecy in the underworld: the knowledge needed to reach home",
            primary: MonomythStage::TheUltimateBoon,
            alternatives: &[],
        },
        StageNode {
            label: "RefusalOfTheReturn",
            hint: "years lingering with Calypso, then with Circe, before departure",
            primary: MonomythStage::RefusalOfTheReturn,
            alternatives: &[],
        },
        StageNode {
            label: "TheMagicFlight",
            hint: "the Phaeacians' enchanted ship carries the sleeping hero home",
            primary: MonomythStage::TheMagicFlight,
            alternatives: &[],
        },
        StageNode {
            label: "RescueFromWithout",
            hint: "the Phaeacians and Athena deliver the helpless hero to Ithaca",
            primary: MonomythStage::RescueFromWithout,
            alternatives: &[],
        },
        StageNode {
            label: "TheCrossingOfTheReturnThreshold",
            hint: "landing on Ithaca disguised as a beggar",
            primary: MonomythStage::TheCrossingOfTheReturnThreshold,
            alternatives: &[],
        },
        StageNode {
            label: "MasterOfTheTwoWorlds",
            hint: "the slaughter of the suitors and Athena-sanctioned reclamation of the kingship",
            primary: MonomythStage::MasterOfTheTwoWorlds,
            alternatives: &[],
        },
        StageNode {
            label: "FreedomToLive",
            hint: "recognition by Penelope and Laertes; Athena brokers peace with the suitors' kin",
            primary: MonomythStage::FreedomToLive,
            alternatives: &[],
        },
    ]
}

fn node_spec(stage_node: &StageNode) -> NodeSpec {
    let mut stage = ScoredOne::new(stage_node.primary);
    for &(alt, weight) in stage_node.alternatives {
        stage.insert_alternative(alt, Weight::new(weight));
    }
    NodeSpec {
        label: stage_node.label.to_owned(),
        stage,
        synopsis_hint: stage_node.hint.to_owned(),
        situation: None,
        functions: monomyth_core::ScoredSet::new(),
        motifs: monomyth_core::ScoredSet::new(),
    }
}

/// A minimal single-node narrative structure to build the Odyssey spine onto: one
/// root beat (the first stage, `CallToAdventure`) that is also, until spliced,
/// the sole ending.
fn seed_structure() -> monomyth_core::NarrativeStructure {
    let spine = odyssey_spine();
    let first = &spine[0];
    let mut nodes = SlotMap::with_key();
    let root_node = {
        let mut node = monomyth_core::NarrativeNode::new(
            first.label,
            monomyth_core::NodeKind::Origin,
            first.primary,
            synopsis(first.hint),
        );
        let mut stage = ScoredOne::new(first.primary);
        for &(alt, weight) in first.alternatives {
            stage.insert_alternative(alt, Weight::new(weight));
        }
        node.stage = stage;
        node
    };
    let root = nodes.insert(root_node);
    monomyth_core::NarrativeStructure {
        nodes,
        root,
        endings: BTreeSet::from([root]),
    }
}

/// Apply the Odyssey spine to a fresh structure, returning the finished
/// [`monomyth_core::World`] and the ordered [`NarrativeEdit`] script that
/// reproduces it (starting from [`seed_structure`]).
///
/// Ids in `InsertBeat`/`Connect`/`MarkEnding` edits are only knowable once the
/// preceding edit has actually run (a fresh [`monomyth_core::NarrativeNodeId`] is
/// a `SlotMap` key assigned at insertion time), so the script is built by
/// applying one [`NarrativeEdit`] at a time and recording the *concrete* edit
/// (with its real, resolved ids) into the returned script — this is what makes
/// `artifacts/benchmarks/greek/odyssey.edits.json` a faithful, replayable
/// transcript rather than a template with placeholder ids.
fn build_world_and_script() -> (World, Vec<NarrativeEdit>) {
    let spine = odyssey_spine();
    let mut structure = seed_structure();
    let root = structure.root();
    let mut script = Vec::new();

    let mut tail = root;
    for stage_node in &spine[1..] {
        let spec = node_spec(stage_node);
        let add_edit = NarrativeEdit::AddNode { spec };
        let EditOutcome::NodeAdded(new_id) = structure
            .apply_edit(&add_edit)
            .expect("AddNode has no local precondition to violate")
        else {
            unreachable!("AddNode always reports NodeAdded");
        };
        script.push(add_edit);

        let connect_edit = NarrativeEdit::Connect {
            source: tail,
            target: new_id,
            kind: EdgeKind::Sequence,
        };
        structure
            .apply_edit(&connect_edit)
            .expect("connecting the freshly added node to the current tail always succeeds");
        script.push(connect_edit);

        tail = new_id;
    }

    // Re-point endings: only the final spine node is a real ending.
    let unmark_root = NarrativeEdit::UnmarkEnding { node: root };
    structure
        .apply_edit(&unmark_root)
        .expect("root exists, so unmarking is a no-op success");
    script.push(unmark_root);

    let mark_tail = NarrativeEdit::MarkEnding { node: tail };
    structure
        .apply_edit(&mark_tail)
        .expect("the final spine node exists");
    script.push(mark_tail);

    structure.recompute_kinds();
    structure
        .validate()
        .expect("the Odyssey spine is a single-source, acyclic, reconverging DAG");

    // Odysseus, the sole named entity: optional per the task, kept minimal.
    let mut locations = SlotMap::with_key();
    let ithaca = locations.insert(Location {
        name: Content::empty(ContentPrompt::new(ContentKind::Name, "Ithaca")),
        description: Content::empty(ContentPrompt::new(
            ContentKind::Description,
            "the hero's home, held for him through his absence",
        )),
        exits: std::collections::BTreeMap::new(),
        entities: BTreeSet::new(),
        items: BTreeSet::new(),
    });

    let mut entities = SlotMap::with_key();
    let odysseus = entities.insert(Entity {
        name: Content::empty(ContentPrompt::new(ContentKind::Name, "Odysseus")),
        description: Content::empty(ContentPrompt::new(
            ContentKind::Description,
            "king of Ithaca, the poem's protagonist",
        )),
        kind: EntityKind::Npc,
        role: Some(ScoredOne::new(ProppRole::Hero)),
        archetype: None,
        location: Some(ithaca),
    });
    locations[ithaca].entities.insert(odysseus);

    let cursor = structure.root();
    let world = World {
        meta: WorldMeta {
            seed: 0,
            schema_version: SCHEMA_VERSION,
            title: Content::empty(ContentPrompt::new(
                ContentKind::Title,
                "The Odyssey — Campbell macro-axis benchmark",
            )),
        },
        locations,
        entities,
        items: SlotMap::with_key(),
        player: Player::new(ithaca),
        story: Story {
            structure,
            plot: None,
            quests: SlotMap::with_key(),
        },
        state: WorldState {
            cursor,
            ..WorldState::default()
        },
        rng: RngState::new(0),
    };

    (world, script)
}

fn artifacts_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/benchmarks/greek")
}

fn committed_world_json() -> String {
    std::fs::read_to_string(artifacts_dir().join("odyssey.json"))
        .expect("artifacts/benchmarks/greek/odyssey.json is committed")
}

fn committed_edits_json() -> String {
    std::fs::read_to_string(artifacts_dir().join("odyssey.edits.json"))
        .expect("artifacts/benchmarks/greek/odyssey.edits.json is committed")
}

/// The builder is regenerable and drift-detecting: re-running it must reproduce
/// exactly the committed `odyssey.json` bytes and the pinned FNV hash. Also
/// verifies the committed edit-script reproduces the same `World` when replayed
/// through `apply_edits`, and that every `Content` slot in the built world is
/// `Content::Empty` (no prose ever baked into the fixture).
#[test]
fn odyssey_builder_should_reproduce_the_committed_fixture_and_pinned_hash() {
    let (world, _script) = build_world_and_script();
    world.validate().expect("the built world is valid");

    let rebuilt_json = serde_json::to_string_pretty(&world).expect("the built world serializes");
    let committed_json = committed_world_json();
    assert_eq!(
        rebuilt_json.trim_end(),
        committed_json.trim_end(),
        "the builder must reproduce artifacts/benchmarks/greek/odyssey.json byte-for-byte"
    );

    let hash = monomyth_eval::util::fnv1a(committed_json.as_bytes());
    assert_eq!(
        hash, ODYSSEY_CONTENT_HASH,
        "the committed fixture's FNV hash must match the pin in artifacts/benchmarks/index.json"
    );
}

/// The committed edit-script, applied to the seed structure via `apply_edit` in
/// order (mirroring how `build_world_and_script` produced it), must reproduce the
/// same narrative structure the committed fixture carries.
#[test]
fn odyssey_edit_script_should_replay_to_the_same_structure() {
    let edits_json = committed_edits_json();
    let script: Vec<NarrativeEdit> =
        serde_json::from_str(&edits_json).expect("the committed edit-script parses");

    let mut structure = seed_structure();
    for edit in &script {
        structure
            .apply_edit(edit)
            .expect("every committed edit applies cleanly in order");
    }
    structure.recompute_kinds();
    structure
        .validate()
        .expect("the replayed structure is valid");

    let committed_json = committed_world_json();
    let committed_world: World =
        serde_json::from_str(&committed_json).expect("the committed fixture parses");

    let replayed_structure_json =
        serde_json::to_string(&structure).expect("the replayed structure serializes");
    let committed_structure_json = serde_json::to_string(&committed_world.story.structure)
        .expect("the committed structure serializes");
    assert_eq!(
        replayed_structure_json, committed_structure_json,
        "replaying the committed edit-script must reproduce the committed structure exactly"
    );
}

/// No `Content` slot anywhere in the fixture may be `Filled` — this is the
/// licensing-hard-invariant check specific to ground-truth fixtures (ADR-0023):
/// structural labels only, never prose, PD or otherwise.
#[test]
fn odyssey_fixture_should_have_every_content_slot_empty() {
    let committed_json = committed_world_json();
    let world: World = serde_json::from_str(&committed_json).expect("fixture parses");

    for node in world.story.structure.nodes.values() {
        assert!(
            !node.synopsis.is_filled(),
            "node '{}' synopsis must stay Content::Empty",
            node.label
        );
        for edge in &node.out {
            assert!(
                !edge.label.is_filled(),
                "an edge label out of node '{}' must stay Content::Empty",
                node.label
            );
        }
    }
    assert!(
        !world.meta.title.is_filled(),
        "title must stay Content::Empty"
    );
    for location in world.locations.values() {
        assert!(!location.name.is_filled());
        assert!(!location.description.is_filled());
    }
    for entity in world.entities.values() {
        assert!(!entity.name.is_filled());
        assert!(!entity.description.is_filled());
    }
}

/// PD-gate registry test (ADR-0023's enforcement): every fixture registered in
/// `artifacts/benchmarks/index.json` must resolve its `source_id` to a
/// `public_domain` / `cc0` / `permissive` ledger tier. This test walks the real
/// index and ledger; the negative case (a `reference`-tier source must be
/// rejected) is exercised separately below against a hand-built fake registry so
/// it does not depend on the ledger ever containing a deliberately-bad entry.
#[test]
fn benchmark_index_sources_must_all_resolve_to_a_ship_safe_pd_tier() {
    let index_json = std::fs::read_to_string(artifacts_dir().join("../index.json"))
        .expect("artifacts/benchmarks/index.json is committed");
    let index: serde_json::Value = serde_json::from_str(&index_json).expect("index.json parses");
    let fixtures = index["fixtures"]
        .as_array()
        .expect("index.json has a fixtures array");
    assert!(
        !fixtures.is_empty(),
        "the index must register at least one fixture"
    );

    let ledger = Ledger::load_embedded().expect("embedded ledger parses");
    for fixture in fixtures {
        let source_id = fixture["source_id"]
            .as_str()
            .expect("every fixture entry names a source_id");
        let entry = ledger.get(source_id).unwrap_or_else(|| {
            panic!("fixture source_id '{source_id}' must be a declared ledger source")
        });
        let pd_safe = matches!(
            entry.tier,
            monomyth_knowledge::Tier::PublicDomain
                | monomyth_knowledge::Tier::Cc0
                | monomyth_knowledge::Tier::Permissive
        );
        assert!(
            pd_safe,
            "fixture source_id '{source_id}' resolves to non-PD-safe tier {:?}",
            entry.tier
        );
    }
}

/// The Odyssey fixture in particular must resolve to the exact `source_id`
/// registered in this test file's constant, keeping the two in sync.
#[test]
fn odyssey_fixture_source_id_resolves_to_a_public_domain_ledger_entry() {
    let ledger = Ledger::load_embedded().expect("embedded ledger parses");
    let entry = ledger
        .get(ODYSSEY_SOURCE_ID)
        .expect("gutenberg_odyssey_butler is declared in corpus/manifest.json");
    assert_eq!(entry.tier, monomyth_knowledge::Tier::PublicDomain);
    assert_eq!(entry.namespace, monomyth_knowledge::Namespace::Ship);
}

/// Negative case: the PD-gate must reject a fixture whose `source_id` resolves to
/// a `reference` / copyright / `NonCommercial` ledger tier. Exercises the same gate
/// logic as `benchmark_index_sources_must_all_resolve_to_a_ship_safe_pd_tier`
/// against a hand-built fake index entry pointing at a real reference-tier ledger
/// source (`perseus`), so the assertion does not depend on ever polluting the
/// real committed index with a bad fixture.
#[test]
fn fake_fixture_with_reference_tier_source_must_be_rejected() {
    let ledger = Ledger::load_embedded().expect("embedded ledger parses");
    let bad_source_id = "perseus";
    let entry = ledger
        .get(bad_source_id)
        .expect("perseus is declared in corpus/manifest.json as a reference-tier source");
    assert_eq!(
        entry.namespace,
        monomyth_knowledge::Namespace::Reference,
        "perseus must be reference-namespace for this negative test to be meaningful"
    );

    let pd_safe = matches!(
        entry.tier,
        monomyth_knowledge::Tier::PublicDomain
            | monomyth_knowledge::Tier::Cc0
            | monomyth_knowledge::Tier::Permissive
    );
    assert!(
        !pd_safe,
        "a fixture pointing at '{bad_source_id}' must be rejected by the PD gate, not accepted"
    );
}

/// `monomyth_eval::Benchmark::load` must accept the real committed fixture at its
/// pinned hash, and reject it at a wrong hash.
#[test]
fn benchmark_load_should_accept_the_odyssey_fixture_at_its_pinned_hash() {
    let json = committed_world_json();
    let benchmark = monomyth_eval::Benchmark::load(&json, ODYSSEY_CONTENT_HASH)
        .expect("the committed fixture loads at its pinned hash");
    assert_eq!(benchmark.content_hash, ODYSSEY_CONTENT_HASH);
    assert_eq!(
        benchmark.world.story.structure.nodes.len(),
        odyssey_spine().len(),
        "the loaded world must carry exactly one node per realized spine stage"
    );
}

#[test]
fn benchmark_load_should_reject_the_odyssey_fixture_at_a_wrong_hash() {
    let json = committed_world_json();
    let wrong_hash = ODYSSEY_CONTENT_HASH.wrapping_add(1);
    let error = monomyth_eval::Benchmark::load(&json, wrong_hash)
        .expect_err("a wrong hash must be rejected even though the JSON is a valid fixture");
    assert!(matches!(
        error,
        monomyth_eval::BenchmarkError::HashMismatch { .. }
    ));
}

/// Not a test: a one-shot generator that (re)writes the committed fixture bytes
/// from the builder above, for use only when intentionally regenerating the
/// fixture. Run explicitly with `cargo test -p monomyth-eval --test
/// odyssey_fixture -- --ignored generate_odyssey_fixture_files`, then re-pin
/// `ODYSSEY_CONTENT_HASH` here and in `artifacts/benchmarks/index.json` from its
/// printed output, and review the diff by hand (ADR-0014's confirmation
/// practice) before committing.
#[test]
#[ignore = "one-shot fixture (re)generator, not part of the regression suite"]
fn generate_odyssey_fixture_files() {
    let (world, script) = build_world_and_script();
    world.validate().expect("built world is valid");

    let world_json = serde_json::to_string_pretty(&world).expect("world serializes");
    let edits_json = serde_json::to_string_pretty(&script).expect("edit-script serializes");

    std::fs::write(
        artifacts_dir().join("odyssey.json"),
        format!("{world_json}\n"),
    )
    .expect("writes odyssey.json");
    std::fs::write(
        artifacts_dir().join("odyssey.edits.json"),
        format!("{edits_json}\n"),
    )
    .expect("writes odyssey.edits.json");

    let hash = monomyth_eval::util::fnv1a(format!("{world_json}\n").as_bytes());
    println!("ODYSSEY_CONTENT_HASH = {hash:#018x}");
}
