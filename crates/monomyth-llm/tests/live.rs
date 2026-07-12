//! Live smoke test against a real provider.
//!
//! Ignored by default so it never runs in CI without credentials. Run manually
//! with a provider key in the environment (the repo `.env` supplies one for the
//! binary; this test relies on the variable already being present):
//!
//! ```sh
//! GEMINI_API_KEY=... cargo test -p monomyth-llm --test live -- --ignored
//! ```
//!
//! Model: `gemini/gemini-3.5-flash` — the project's configured provider/tier for
//! cheap structured-output calls.

use monomyth_llm::Llm;
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema, PartialEq, Eq)]
struct Greeting {
    /// A single friendly word.
    word: String,
}

#[tokio::test]
#[ignore = "requires a live provider API key in the environment"]
async fn generates_a_trivial_value_against_a_live_model() {
    let llm = Llm::from_env("gemini/gemini-3.5-flash").expect("model string is non-empty");

    let greeting = llm
        .generate::<Greeting>(
            "Return a JSON object whose `word` field is the single word \"hello\".",
            "greeting",
        )
        .await
        .expect("live generation should succeed");

    assert_eq!(
        greeting.value.word.to_lowercase(),
        "hello",
        "the model was asked to return the single word \"hello\""
    );
}
