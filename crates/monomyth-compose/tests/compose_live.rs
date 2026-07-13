//! Records a compose-pipeline cassette against live Gemini.
//!
//! Ignored by default: it makes a real network call and costs real tokens. Run
//! it manually to (re-)record `tests/cassettes/compose_seed42_gemini.json`:
//!
//! ```sh
//! cargo test -p monomyth-compose --test compose_live -- --ignored
//! ```
//!
//! The cassette is what `compose_replay.rs` replays offline in CI. Compose
//! makes only `Continuation` LLM calls (schema name `compose_continuation`); the
//! cassette keys on `(schema_name, prompt)`. Any change to the seed-42 world (the
//! procedural generator's default passes), the compose prompt templates in
//! `src/prompts.rs`, or `support::bounded_compose_settings` invalidates those
//! keys and requires re-recording both this test and `compose_replay` together.

mod support;

use monomyth_compose::{compose, plan::plan};
use monomyth_gen::Generator;
use monomyth_llm::{BackendOptions, Llm, RecordingBackend, XbergBackend};
use support::{MODEL, SEED, bounded_compose_settings, reference_knowledge_with_fixed_passage};

#[tokio::test]
#[ignore = "hits live Gemini; run manually to record the cassette"]
async fn records_a_compose_cassette_against_live_gemini() {
    dotenvy::dotenv().ok();

    let cassette_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/cassettes/compose_seed42_gemini.json"
    );

    // Recording makes one live call per outline section in sequence, so a single
    // transient transport blip anywhere along the way aborts the whole run and
    // leaves a partial cassette. Grant more transport retries and a longer
    // timeout than the hardened default here — this affects only the recording,
    // never the committed cassette or the deterministic offline replay.
    let options = BackendOptions {
        max_retries: Some(6),
        timeout_secs: Some(120),
        ..BackendOptions::default()
    };
    let backend = RecordingBackend::new(
        XbergBackend::with_options(MODEL, options),
        MODEL,
        cassette_path,
    );
    let llm = Llm::new(Box::new(backend));

    let world = Generator::with_default_passes()
        .generate_structure(SEED)
        .expect("the default pipeline generates a world");
    let outline = plan(&world).expect("a generated world is composable");
    let knowledge = reference_knowledge_with_fixed_passage().await;

    let doc = compose(&llm, &knowledge, &world, &bounded_compose_settings())
        .await
        .expect("compose succeeds against live Gemini");

    assert_eq!(doc.sections().len(), outline.len());
    assert!(!doc.prose().is_empty());

    // Force the `RecordingBackend`'s drop-flush before inspecting the cassette
    // file it wrote.
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
