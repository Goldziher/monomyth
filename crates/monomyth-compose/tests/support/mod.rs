//! Shared test doubles and fixtures for the compose pipeline's live/replay
//! cassette integration tests (`compose_live.rs`, `compose_replay.rs`).
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
use monomyth_compose::ComposeSettings;
use monomyth_knowledge::rag::pipeline::Embedder;
use monomyth_knowledge::rag::{InMemoryVectorStore, RagResult, VectorStore};
use monomyth_knowledge::{EMBEDDING_DIM, IngestInput, Knowledge, Ledger};

/// The seed the recorded cassette is keyed against; must match `compose_live.rs`
/// and `compose_replay.rs`.
pub const SEED: u64 = 42;

/// The `"provider/model"` routing string the cassette is recorded against.
///
/// Pinned to `[models].compose`'s system default
/// (`monomyth_config::schema::DEFAULT_COMPOSE_MODEL`). If that default changes,
/// this cassette goes stale — the `(schema_name, prompt)` keys it was recorded
/// under no longer match what a re-run would issue against the new default — and
/// must be re-recorded via
/// `cargo test -p monomyth-compose --test compose_live -- --ignored`.
pub const MODEL: &str = "gemini/gemini-3.5-flash";

/// A declared `reference`-tier source id in `corpus/manifest.json`, used to
/// ground the compose pipeline's REFERENCE-path retrieval. Compose never reads
/// the `ship` namespace: see `monomyth_compose`'s module docs and the
/// licensing-and-provenance invariant.
pub const REFERENCE_SOURCE_ID: &str = "perseus";

/// The fixed passage [`reference_knowledge_with_fixed_passage`] ingests under
/// [`REFERENCE_SOURCE_ID`].
///
/// Fixed and deterministic so a live-recorded cassette and a later replay ingest
/// byte-identical grounding text, and therefore issue byte-identical prompts.
/// Thematically neutral (a hero's transformative return) so it grounds any
/// Campbell stage the seed-42 outline visits.
const RECORDED_PASSAGE: &str =
    "The hero returns across the threshold bearing a boon that transforms the world left behind.";

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
/// [`RECORDED_PASSAGE`] ingested under [`REFERENCE_SOURCE_ID`] via
/// [`Knowledge::ingest_reference`] — never [`Knowledge::ingest`]. Ship-safe: this
/// passage is reference-only and therefore never surfaceable, matching how the
/// compose pipeline itself grounds (`KnowledgeQuery::reference` only).
pub async fn reference_knowledge_with_fixed_passage() -> Knowledge {
    let store: Arc<dyn VectorStore> =
        Arc::new(InMemoryVectorStore::new("monomyth-compose-cassette"));
    let embedder: Arc<dyn Embedder> = Arc::new(FakeEmbedder);
    let ledger = Ledger::load_embedded().expect("embedded manifest parses");
    let knowledge = Knowledge::with(store, embedder, ledger);
    knowledge
        .ingest_reference(REFERENCE_SOURCE_ID, IngestInput::new(RECORDED_PASSAGE))
        .await
        .expect("the fixed reference passage ingests");
    knowledge
}

/// [`ComposeSettings`] bounded to exactly one LLM call per outline section.
///
/// `max_turns: 1` caps each section to a single continuation turn.
/// `revise_threshold: -1.0` guarantees the first attempt's cosine score (always
/// `>= -1.0`) meets threshold, so the revise loop never issues a second call.
/// Together these keep a live recording (and its replay) to exactly one
/// `compose_continuation` call per outline section.
#[must_use]
pub fn bounded_compose_settings() -> ComposeSettings {
    ComposeSettings {
        max_turns: 1,
        grounding_top_k: 2,
        revise_threshold: -1.0,
        max_revise_iterations: 0,
    }
}
