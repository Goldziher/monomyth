//! Shared test doubles and assertion helpers for the content-fill integration
//! tests (`content.rs`, `live_fill.rs`, `replay_fill.rs`).
//!
//! A `tests/support/mod.rs` is not itself compiled as a test binary (it has no
//! `#[test]`); each test file opts in with `mod support;`. Cargo compiles this
//! module as a fresh crate root per integration-test binary, so an item used by
//! one binary but not another is genuinely flagged `dead_code`/`unreachable_pub`
//! in the binary that doesn't use it — not because it is actually unused.
#![allow(
    dead_code,
    unreachable_pub,
    reason = "shared module compiled per test binary; see above"
)]

use std::sync::Arc;

use async_trait::async_trait;
use monomyth_core::{Content, ProvenanceSource, World};
use monomyth_knowledge::rag::pipeline::Embedder;
use monomyth_knowledge::rag::{InMemoryVectorStore, RagResult};
use monomyth_knowledge::{EMBEDDING_DIM, IngestInput, Knowledge, Ledger};

/// A ship-namespace source in the embedded ledger, used both to prove grounding
/// provenance names its source and to seed [`record_knowledge`]'s fixed passage.
/// Coupled to that ledger entry existing.
pub const TEST_SOURCE_ID: &str = "polti";

/// The fixed passage [`record_knowledge`] ingests under [`TEST_SOURCE_ID`].
///
/// Fixed and deterministic so a live-recorded cassette and a later replay ingest
/// byte-identical grounding text, and therefore issue byte-identical prompts.
const RECORDED_PASSAGE: &str =
    "The Suppliant implores a Power in authority; the mediator stands between two poles.";

/// A deterministic, content-derived embedder of the collection dimension — mirrors
/// the pattern in `monomyth-knowledge`'s own tests. No ONNX, no network.
#[derive(Debug)]
pub struct FakeEmbedder;

#[async_trait]
impl Embedder for FakeEmbedder {
    async fn embed(&self, texts: Vec<String>) -> RagResult<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| deterministic_vector(text))
            .collect())
    }
}

/// A deterministic, content-derived embedding vector: no ONNX, no network.
#[must_use]
pub fn deterministic_vector(text: &str) -> Vec<f32> {
    let width = EMBEDDING_DIM as usize;
    let mut vector = vec![0.0f32; width];
    for (index, byte) in text.bytes().enumerate() {
        vector[index % width] += f32::from(byte) / 255.0;
    }
    vector
}

/// A real knowledge layer over an in-memory store and the fake embedder, with
/// nothing ingested.
#[must_use]
pub fn test_knowledge() -> Knowledge {
    let store: Arc<dyn monomyth_knowledge::rag::VectorStore> =
        Arc::new(InMemoryVectorStore::new("test"));
    let embedder: Arc<dyn Embedder> = Arc::new(FakeEmbedder);
    let ledger = Ledger::load_embedded().expect("embedded manifest parses");
    Knowledge::with(store, embedder, ledger)
}

/// A real knowledge layer with one fixed ship passage ingested under
/// [`TEST_SOURCE_ID`], so grounding is exercised identically for a live recording
/// and its later replay. Ship-only: never ingests a reference source.
pub async fn record_knowledge() -> Knowledge {
    let knowledge = test_knowledge();
    knowledge
        .ingest(TEST_SOURCE_ID, IngestInput::new(RECORDED_PASSAGE))
        .await
        .expect("the fixed ship passage ingests");
    knowledge
}

/// Every content slot the default content pipeline is responsible for.
#[must_use]
pub fn targeted_slots(world: &World) -> Vec<&Content> {
    let mut slots = vec![&world.meta.title];
    for location in world.locations.values() {
        slots.push(&location.name);
        slots.push(&location.description);
    }
    for entity in world.entities.values() {
        slots.push(&entity.name);
        slots.push(&entity.description);
    }
    for item in world.items.values() {
        slots.push(&item.name);
        slots.push(&item.description);
    }
    slots
}

/// Assert every targeted slot in `world` is filled with LLM provenance naming
/// `expected_model`.
pub fn assert_all_slots_filled_with_llm_provenance(world: &World, expected_model: &str) {
    for slot in targeted_slots(world) {
        assert!(slot.is_filled(), "every targeted slot must be filled");
        let provenance = slot.provenance().expect("a filled slot has provenance");
        match &provenance.source {
            ProvenanceSource::Llm { model } => {
                assert_eq!(
                    model, expected_model,
                    "provenance must carry the context model label"
                );
            }
            ProvenanceSource::Procedural => {
                panic!("expected an LLM provenance source, got Procedural")
            }
        }
    }
}
