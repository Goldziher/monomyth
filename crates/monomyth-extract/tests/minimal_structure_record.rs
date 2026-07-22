//! Records the `minimal_structure_beat.json` cassette
//! [`minimal_structure_extraction.rs`](minimal_structure_extraction) replays
//! offline.
//!
//! Ignored by default. Unlike `monomyth-compose`'s live-recording tests, this
//! one never touches the network or a real provider: `MinimalStructureExtractor`
//! only needs a [`StructuredBackend`], so the "recording" here wraps a small
//! scripted in-process fake (a stand-in for a real model's JSON output) rather
//! than a live backend. That keeps this repo's whole `monomyth-extract` test
//! surface hermetic — including the one-time act of producing the cassette — at
//! the cost of the cassette not reflecting a real model's behavior. Run it
//! manually to (re-)record `tests/cassettes/minimal_structure_beat.json`:
//!
//! ```sh
//! cargo test -p monomyth-extract --test minimal_structure_record -- --ignored
//! ```
//!
//! The cassette keys on `(schema_name, prompt)`. Any change to
//! `support::SOURCE_TEXT`, `MinimalStructureExtractor`'s prompt template, or the
//! Campbell stage catalog (`artifacts/frameworks/campbell_monomyth.json`)
//! invalidates that key and requires re-recording both this test and
//! `minimal_structure_extraction.rs` together.

mod support;

use async_trait::async_trait;
use monomyth_contracts::StructureExtractor as _;
use monomyth_extract::MinimalStructureExtractor;
use monomyth_llm::{BackendError, Llm, RecordingBackend, StructuredBackend, Usage};
use serde_json::{Value, json};
use support::{MODEL, SOURCE_TEXT};

/// A scripted stand-in for a live model: always returns the same canned
/// `ExtractedBeat`-shaped JSON, regardless of the prompt it is called with.
struct ScriptedBeatBackend;

#[async_trait]
impl StructuredBackend for ScriptedBeatBackend {
    async fn complete_json(
        &self,
        _prompt: &str,
        _schema_name: &str,
        _schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError> {
        Ok((
            json!({
                "node_label": "CallToAdventure",
                "stage_id": 1,
                "synopsis_hint": "A herald tells the smith she alone bears the mark that can open the mountain gate.",
                "location_name": "The Quiet Village",
                "location_description": "A small settlement at the foot of the mountain, its smithy still warm from the day's work.",
            }),
            None,
        ))
    }

    async fn complete_text(&self, _prompt: &str) -> Result<(String, Option<Usage>), BackendError> {
        Err(BackendError::new(
            "text completion is not used by MinimalStructureExtractor",
        ))
    }
}

#[tokio::test]
#[ignore = "regenerates the committed cassette; run manually after changing the prompt or schema"]
async fn records_a_minimal_structure_cassette() {
    let cassette_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/cassettes/minimal_structure_beat.json"
    );
    let backend = RecordingBackend::new(ScriptedBeatBackend, MODEL, cassette_path);
    let llm = Llm::new(Box::new(backend));

    let extractor = MinimalStructureExtractor::new(&llm, MODEL);
    let world = extractor
        .extract_structure(SOURCE_TEXT)
        .await
        .expect("extraction against the scripted backend succeeds");
    world.validate().expect("the extracted world is valid");

    // Force the `RecordingBackend`'s drop-flush before inspecting the cassette ~keep
    // file it wrote, mirroring `monomyth-compose`'s `compose_live.rs`. ~keep
    drop(llm);

    let cassette = std::fs::read_to_string(cassette_path).expect("the cassette file was written");
    assert!(!cassette.is_empty(), "the cassette must not be empty");
}
