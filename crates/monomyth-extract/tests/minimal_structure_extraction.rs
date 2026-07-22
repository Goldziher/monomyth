//! Replays the `minimal_structure_beat.json` cassette recorded by
//! `minimal_structure_record.rs`.
//!
//! This is the hermetic, offline test that exercises
//! [`MinimalStructureExtractor`] end to end: no network, no provider key,
//! byte-identical output across runs. It proves the roadmap's Phase 3 de-risk
//! goal directly — that the frozen contract (`monomyth-core`'s `World`,
//! written exclusively through `NarrativeEdit` plus the unavoidable
//! `Location`/root bootstrap the module doc explains) can hold *extracted*
//! structure derived from raw source text, not just a re-classified label.

mod support;

use monomyth_contracts::StructureExtractor as _;
use monomyth_core::Content;
use monomyth_extract::MinimalStructureExtractor;
use monomyth_llm::{Llm, ReplayBackend};
use support::{MODEL, SOURCE_TEXT};

#[tokio::test]
async fn replays_the_minimal_structure_cassette_offline() {
    let backend = ReplayBackend::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/cassettes/minimal_structure_beat.json"
    ))
    .expect("cassette loads");
    let llm = Llm::new(Box::new(backend));

    let extractor = MinimalStructureExtractor::new(&llm, MODEL);
    let world = extractor
        .extract_structure(SOURCE_TEXT)
        .await
        .expect("extraction succeeds against the replayed cassette");

    world
        .validate()
        .expect("the extracted world passes World::validate, the write surface's gate");

    // Exactly one node, exactly one location — the minimal slice's contract. ~keep
    assert_eq!(world.story.structure.nodes.len(), 1);
    assert_eq!(world.locations.len(), 1);
    assert!(world.entities.is_empty());
    assert!(world.items.is_empty());
    assert!(world.story.quests.is_empty());

    let root = world.story.structure.root();
    let node = world
        .story
        .structure
        .node(root)
        .expect("the root node exists");
    assert_eq!(node.label, "CallToAdventure");
    assert_eq!(
        *node.stage.primary(),
        monomyth_frameworks::MonomythStage::CallToAdventure
    );
    assert_eq!(
        node.synopsis.prompt().hint,
        "A herald tells the smith she alone bears the mark that can open the mountain gate."
    );
    assert_eq!(world.story.structure.endings().collect::<Vec<_>>(), [root]);
    assert_eq!(world.state.cursor, root);

    let (_, location) = world
        .locations
        .iter()
        .next()
        .expect("exactly one location exists");
    assert_eq!(
        location.name.value().map(String::as_str),
        Some("The Quiet Village")
    );
    assert_eq!(
        location.description.value().map(String::as_str),
        Some(
            "A small settlement at the foot of the mountain, its smithy still warm from the \
             day's work."
        )
    );
    assert!(matches!(location.name, Content::Filled { .. }));
    assert!(matches!(location.description, Content::Filled { .. }));

    // The gold snapshot: extraction is a pure function of (text, cassette), so ~keep
    // the serialized world must stay byte-identical to the committed fixture. ~keep
    // `SlotMap` serializes deterministically by insertion order (see ~keep
    // `monomyth-core::world`'s docs), and exactly one location and one node are ~keep
    // ever inserted, in fixed order, so this snapshot is stable across runs. ~keep
    let serialized = serde_json::to_string_pretty(&world).expect("world serializes");
    let gold = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/minimal_structure.gold.json"
    ))
    .expect("the gold fixture is committed");
    assert_eq!(
        serialized.trim_end(),
        gold.trim_end(),
        "extracted world drifted from the committed gold snapshot"
    );
}
