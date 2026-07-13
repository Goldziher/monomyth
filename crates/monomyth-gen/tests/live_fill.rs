//! Records a content-fill cassette against live Gemini.
//!
//! Ignored by default: it makes a real network call and costs real tokens. Run
//! it manually to (re-)record `tests/cassettes/pipeline_seed42_gemini.json`:
//!
//! ```sh
//! cargo test -p monomyth-gen --test live_fill -- --ignored
//! ```
//!
//! The cassette is what `replay_fill.rs` replays offline in CI. If the seed-42
//! structure or the content passes' prompt strings change, the cassette keys go
//! stale and both this test and `replay_fill` must be re-recorded together.

mod support;

use monomyth_gen::{ContentConfig, ContentContext, Generator};
use monomyth_llm::{BackendOptions, Llm, RecordingBackend, XbergBackend};
use support::{assert_all_slots_filled_with_llm_provenance, record_knowledge};

/// The seed the recorded cassette is keyed against; must match `replay_fill.rs`.
const SEED: u64 = 42;
/// The `"provider/model"` routing string the cassette is recorded against.
const MODEL: &str = "gemini/gemini-3.5-flash";

#[tokio::test]
#[ignore = "hits live Gemini; run manually to record the cassette"]
async fn records_a_content_fill_cassette_against_live_gemini() {
    dotenvy::dotenv().ok();

    let cassette_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/cassettes/pipeline_seed42_gemini.json"
    );

    let backend = RecordingBackend::new(
        XbergBackend::with_options(MODEL, BackendOptions::default()),
        MODEL,
        cassette_path,
    );
    let llm = Llm::new(Box::new(backend));

    let generator = Generator::with_default_passes();
    let mut world = generator
        .generate_structure(SEED)
        .expect("the default pipeline generates a world");
    let knowledge = record_knowledge().await;
    let context = ContentContext {
        llm: &llm,
        knowledge: &knowledge,
        model: MODEL,
        config: &ContentConfig::default(),
    };

    generator
        .fill_content(&mut world, &context)
        .await
        .expect("content fill succeeds against live Gemini");

    assert_all_slots_filled_with_llm_provenance(&world, MODEL);

    // Force the `RecordingBackend`'s drop-flush before inspecting the cassette ~keep
    // file it wrote. ~keep
    drop(llm);

    let cassette = std::fs::read_to_string(cassette_path).expect("the cassette file was written");
    assert!(!cassette.is_empty(), "the cassette must not be empty");
    for marker in ["api_key", "sk-", "AIza"] {
        assert!(
            !cassette.contains(marker),
            "the cassette must never contain the secret marker '{marker}'"
        );
    }
}
