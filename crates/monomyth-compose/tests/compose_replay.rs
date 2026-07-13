//! Replays the compose-pipeline cassette recorded from live Gemini by
//! `compose_live.rs`.
//!
//! This is the offline, CI-safe counterpart to `compose_live.rs`: no network, no
//! provider key, byte-identical output across runs. It only works if the
//! cassette at `tests/cassettes/compose_seed42_gemini.json` exists and its
//! recorded prompts key-match what this test issues.
//!
//! The replay reconstructs the exact same inputs the live recording used: the
//! same seed (`42`), the same fixed reference passage via
//! [`support::reference_knowledge_with_fixed_passage`], the same bounded
//! settings, and the same model label. If the seed-42 procedural structure
//! changes, or the compose prompt templates change, the `(schema_name, prompt)`
//! cassette keys will miss and this test must be re-recorded via
//! `cargo test -p monomyth-compose --test compose_live -- --ignored`.

mod support;

use monomyth_compose::{compose, plan::plan};
use monomyth_gen::Generator;
use monomyth_llm::{Llm, ReplayBackend};
use support::{SEED, bounded_compose_settings, reference_knowledge_with_fixed_passage};

#[tokio::test]
async fn replays_the_compose_cassette_offline() {
    let backend = ReplayBackend::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/cassettes/compose_seed42_gemini.json"
    ))
    .expect("cassette loads");
    let llm = Llm::new(Box::new(backend));

    let world = Generator::with_default_passes()
        .generate_structure(SEED)
        .expect("the default pipeline generates a world");
    let outline = plan(&world).expect("a generated world is composable");
    let knowledge = reference_knowledge_with_fixed_passage().await;

    let doc = compose(&llm, &knowledge, &world, &bounded_compose_settings())
        .await
        .expect("compose succeeds against the replayed cassette");

    assert_eq!(doc.sections().len(), outline.len());
    assert!(!doc.prose().is_empty());
}
