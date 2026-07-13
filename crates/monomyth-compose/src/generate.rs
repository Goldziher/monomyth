//! The multi-turn long-form generation primitive.
//!
//! [`generate_long_form`] is the primitive the (future) Draft phase calls once
//! per outline section: it asks the model for prose in a loop, accumulating
//! each turn's text, until the model reports it is done or a caller-supplied
//! turn cap is reached. It is the *only* thing in this slice that touches an
//! LLM — no retrieval, no revise loop, no cassette. Prose length is therefore
//! variable (model-driven, via [`Continuation::is_complete`]) but always
//! hard-bounded by `max_turns`. It never touches the procedural RNG: the
//! content half of generation is quarantined from the structural half, per the
//! engine's hybrid-generation split.

use monomyth_llm::{Generated, Llm};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::ComposeError;
use crate::prompts;

/// The `schemars`/`Llm::generate` schema name for [`Continuation`].
const CONTINUATION_SCHEMA_NAME: &str = "compose_continuation";

/// A sensible default turn cap for callers that don't need a bespoke one.
///
/// Not enforced internally — [`generate_long_form`] always uses the `max_turns`
/// its caller supplies. This constant exists so call sites and tests have a
/// documented, named default instead of an inline magic number.
pub const DEFAULT_MAX_TURNS: usize = 5;

/// One turn's model output: a chunk of prose plus a signal for whether the
/// section is finished.
///
/// The prompt sent for each turn is built by [`crate::prompts::continuation_prompt`]
/// (prompt role [`crate::prompts::PromptRole::Continuation`]).
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct Continuation {
    /// The prose generated this turn, to be appended to the accumulated text.
    pub text: String,
    /// Whether the model considers the section complete after this turn. When
    /// `true`, [`generate_long_form`] stops even if turns remain under the cap.
    pub is_complete: bool,
}

/// Generate long-form prose across up to `max_turns` model calls, stopping
/// early once the model reports [`Continuation::is_complete`].
///
/// Each turn builds a prompt from `instruction`, the `grounding` passages, and
/// the prose accumulated so far (`prior` on the first turn, then `prior` plus
/// every turn generated since). The returned `String` is the full accumulated
/// prose, `prior` not included (callers that want the combined text
/// concatenate `prior` and the result themselves, keeping this primitive a
/// pure "what did this call add" function).
///
/// # Errors
///
/// Returns [`ComposeError::NoTurns`] if `max_turns == 0` — a zero-turn
/// generation can never produce content, so this is treated as caller misuse
/// rather than silently returning an empty string. Returns
/// [`ComposeError::Generation`] if any underlying [`Llm::generate`] call fails.
pub async fn generate_long_form(
    llm: &Llm,
    instruction: &str,
    grounding: &[String],
    prior: &str,
    max_turns: usize,
) -> Result<String, ComposeError> {
    if max_turns == 0 {
        return Err(ComposeError::NoTurns);
    }

    let mut accumulated = String::new();
    for _turn in 0..max_turns {
        let prompt = prompts::continuation_prompt(instruction, grounding, prior, &accumulated);
        let Generated { value, .. } = llm
            .generate::<Continuation>(&prompt, CONTINUATION_SCHEMA_NAME)
            .await?;

        append_turn(&mut accumulated, &value.text);

        if value.is_complete {
            break;
        }
    }

    Ok(accumulated)
}

/// Append a turn's text to the accumulator, separating turns with a blank line
/// so prose from distinct turns never runs together mid-sentence.
fn append_turn(accumulated: &mut String, turn_text: &str) {
    if !accumulated.is_empty() && !turn_text.is_empty() {
        accumulated.push_str("\n\n");
    }
    accumulated.push_str(turn_text);
}

#[cfg(test)]
mod tests {
    use super::{Continuation, DEFAULT_MAX_TURNS, generate_long_form};
    use crate::error::ComposeError;
    use crate::test_support::{FakeBackend, llm_with};

    #[tokio::test]
    async fn should_concatenate_turns_until_model_reports_complete() {
        let script = vec![
            Continuation {
                text: "Alpha".to_owned(),
                is_complete: false,
            },
            Continuation {
                text: "Omega".to_owned(),
                is_complete: true,
            },
        ];
        let backend = FakeBackend::scripted(script);
        let calls = backend.call_count();
        let llm = llm_with(backend);

        let prose = generate_long_form(&llm, "narrate the departure", &[], "", 5)
            .await
            .expect("generation should succeed within the turn cap");

        assert_eq!(
            prose, "Alpha\n\nOmega",
            "turns must be concatenated in call order, separated by a blank line"
        );
        assert_eq!(
            *calls.lock().expect("lock poisoned"),
            2,
            "generation must stop as soon as is_complete is reported, not run to the cap"
        );
    }

    #[tokio::test]
    async fn should_stop_at_max_turns_when_model_never_completes() {
        const MAX_TURNS: usize = 3;
        let script = vec![
            Continuation {
                text: "Beat".to_owned(),
                is_complete: false,
            };
            MAX_TURNS
        ];
        let backend = FakeBackend::scripted(script);
        let calls = backend.call_count();
        let llm = llm_with(backend);

        let prose = generate_long_form(&llm, "narrate the trial", &[], "", MAX_TURNS)
            .await
            .expect("generation should stop at the cap rather than erroring");

        assert_eq!(
            prose, "Beat\n\nBeat\n\nBeat",
            "exactly max_turns turns must be concatenated when completion is never signalled"
        );
        assert_eq!(
            *calls.lock().expect("lock poisoned"),
            MAX_TURNS,
            "generation must stop after exactly max_turns calls"
        );
    }

    #[tokio::test]
    async fn should_error_on_zero_max_turns() {
        let backend = FakeBackend::scripted(Vec::new());
        let calls = backend.call_count();
        let llm = llm_with(backend);

        let error = generate_long_form(&llm, "narrate the return", &[], "", 0)
            .await
            .expect_err("zero turns can never produce content");

        assert!(
            matches!(error, ComposeError::NoTurns),
            "zero max_turns must be reported as ComposeError::NoTurns, got: {error:?}"
        );
        assert_eq!(
            *calls.lock().expect("lock poisoned"),
            0,
            "a zero-turn misuse must fail before any backend call is made"
        );
    }

    #[tokio::test]
    async fn should_use_default_max_turns_constant_as_a_valid_cap() {
        let script = vec![Continuation {
            text: "Threshold".to_owned(),
            is_complete: true,
        }];
        let backend = FakeBackend::scripted(script);
        let llm = llm_with(backend);

        let prose = generate_long_form(&llm, "narrate the threshold", &[], "", DEFAULT_MAX_TURNS)
            .await
            .expect("DEFAULT_MAX_TURNS must be usable as-is by a caller");

        assert_eq!(prose, "Threshold");
    }
}
