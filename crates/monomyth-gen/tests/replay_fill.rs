//! Replays the content-fill cassette recorded from live Gemini by `live_fill.rs`.
//!
//! This is the offline, CI-safe counterpart to `live_fill.rs`: no network, no
//! provider key, byte-identical output across runs. It only works if the
//! cassette at `tests/cassettes/pipeline_seed42_gemini.json` exists and its
//! recorded prompts key-match what this test issues.
//!
//! The replay reconstructs the exact same inputs the live recording used: the
//! same seed (`42`), the same fixed grounding passage via
//! [`support::record_knowledge`], and the same model label. If the seed-42
//! procedural structure changes, or any content pass's prompt template changes,
//! the `(schema_name, prompt)` cassette keys will miss and this test must be
//! re-recorded via `cargo test -p monomyth-gen --test live_fill -- --ignored`.

mod support;

use monomyth_gen::{ContentConfig, ContentContext, Generator};
use monomyth_llm::{Llm, ReplayBackend};
use support::{assert_all_slots_filled_with_llm_provenance, record_knowledge};

/// The seed the cassette was recorded against; must match `live_fill.rs`.
const SEED: u64 = 42;
/// The `"provider/model"` routing string the cassette was recorded against.
const MODEL: &str = "gemini/gemini-3.5-flash";

#[tokio::test]
async fn replays_the_content_fill_cassette_offline() {
    let backend = ReplayBackend::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/cassettes/pipeline_seed42_gemini.json"
    ))
    .expect("cassette loads");
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
        .expect("content fill succeeds against the replayed cassette");

    assert_all_slots_filled_with_llm_provenance(&world, MODEL);
}
