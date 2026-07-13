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
/// This is the sole prompt role this slice has (a single "continue the prose"
/// instruction), so no template abstraction is introduced yet — that arrives
/// once the Draft phase needs multiple distinct roles (e.g. outline-to-prose
/// vs. revise) in a later slice.
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
        let prompt = build_prompt(instruction, grounding, prior, &accumulated);
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

/// Build the prompt for one turn: the instruction, the grounding passages
/// (labelled and joined), and the prose accumulated so far.
///
/// Kept as a simple, documented private helper rather than a template
/// abstraction — this slice has exactly one prompt role. A template
/// abstraction arrives once the Draft phase needs multiple distinct roles.
fn build_prompt(
    instruction: &str,
    grounding: &[String],
    prior: &str,
    generated_so_far: &str,
) -> String {
    use std::fmt::Write as _;

    let mut prompt = String::new();
    let _ = writeln!(prompt, "Instruction: {instruction}");

    if !grounding.is_empty() {
        prompt.push_str("\nGrounding:\n");
        for passage in grounding {
            let _ = writeln!(prompt, "- {passage}");
        }
    }

    let so_far = combined_prose(prior, generated_so_far);
    if so_far.is_empty() {
        prompt.push_str("\nProse so far: (none; this is the opening turn)\n");
    } else {
        let _ = writeln!(prompt, "\nProse so far:\n{so_far}");
    }

    prompt.push_str(
        "\nContinue the prose from where it leaves off. Report the new text you are adding \
         (not the prose so far) and whether the section is now complete.",
    );
    prompt
}

/// Join `prior` and `generated_so_far` for display in the prompt, treating
/// either half as optional.
fn combined_prose(prior: &str, generated_so_far: &str) -> String {
    match (prior.is_empty(), generated_so_far.is_empty()) {
        (true, true) => String::new(),
        (true, false) => generated_so_far.to_owned(),
        (false, true) => prior.to_owned(),
        (false, false) => format!("{prior}\n\n{generated_so_far}"),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use monomyth_llm::{BackendError, StructuredBackend, Usage};
    use serde_json::Value;

    use super::{Continuation, DEFAULT_MAX_TURNS, generate_long_form};
    use crate::error::ComposeError;

    /// A shared call counter, cloned out of a [`FakeBackend`] before it is moved
    /// into an [`monomyth_llm::Llm`], so tests can assert exactly how many
    /// backend calls a `generate_long_form` invocation made.
    type CallCount = Arc<Mutex<usize>>;

    /// A fake [`StructuredBackend`] that plays back a fixed script of
    /// [`Continuation`] responses in order, recording how many calls it served.
    ///
    /// `complete_text` is unused by [`generate_long_form`] (which only calls
    /// `Llm::generate`), so it is left unimplemented rather than scripted.
    struct FakeBackend {
        script: Mutex<Vec<Continuation>>,
        calls: CallCount,
    }

    impl FakeBackend {
        fn scripted(responses: Vec<Continuation>) -> Self {
            Self {
                script: Mutex::new(responses),
                calls: CallCount::default(),
            }
        }

        /// A shared handle to the call counter, cloned out before the backend is
        /// moved into an [`monomyth_llm::Llm`].
        fn call_count(&self) -> CallCount {
            Arc::clone(&self.calls)
        }
    }

    #[async_trait]
    impl StructuredBackend for FakeBackend {
        async fn complete_json(
            &self,
            _prompt: &str,
            _schema_name: &str,
            _schema: &Value,
        ) -> Result<(Value, Option<Usage>), BackendError> {
            *self.calls.lock().expect("lock poisoned") += 1;
            let mut script = self.script.lock().expect("lock poisoned");
            if script.is_empty() {
                return Err(BackendError::new("no scripted responses remain"));
            }
            let next = script.remove(0);
            Ok((
                serde_json::to_value(next).expect("Continuation serializes"),
                None,
            ))
        }

        async fn complete_text(
            &self,
            _prompt: &str,
        ) -> Result<(String, Option<Usage>), BackendError> {
            unimplemented!("generate_long_form only calls Llm::generate, never Llm::text")
        }
    }

    fn llm_with(backend: FakeBackend) -> monomyth_llm::Llm {
        monomyth_llm::Llm::new(Box::new(backend))
    }

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
