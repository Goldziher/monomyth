//! The typed generation facade.
//!
//! [`Llm`] wraps an erased [`StructuredBackend`] and exposes
//! [`Llm::generate`], a generic over any `DeserializeOwned + JsonSchema` type,
//! plus a plain-[`text`](Llm::text) escape hatch. Backend errors are wrapped
//! with context; malformed model output is retried a bounded number of times.

use std::fmt;

use schemars::{JsonSchema, schema_for};
use serde::de::DeserializeOwned;

use crate::backend::{StructuredBackend, Usage, XbergBackend};
use crate::error::LlmError;

/// Number of retries granted after the first attempt when model output fails to
/// deserialize. Total attempts is therefore `MAX_RETRIES + 1`.
const MAX_RETRIES: u32 = 2;

/// Total number of `complete_json` attempts before [`LlmError::Parse`] is
/// returned. Exposed so downstream matchers on [`LlmError::Parse`] can compare
/// `attempts` against it rather than hardcoding the count.
pub const MAX_ATTEMPTS: u32 = MAX_RETRIES + 1;

/// A typed structured-generation client over a [`StructuredBackend`].
///
/// The backend is type-erased so downstream `&Llm` signatures stay free of a
/// backend type parameter.
pub struct Llm {
    backend: Box<dyn StructuredBackend>,
}

impl fmt::Debug for Llm {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Llm").finish_non_exhaustive()
    }
}

/// A successfully generated value paired with the call's usage metadata.
#[derive(Debug, Clone)]
#[must_use = "a generated result should be consumed; read `.value` (and `.usage`)"]
pub struct Generated<T> {
    /// The deserialized, schema-validated value.
    pub value: T,
    /// Token usage for the generating call, when the provider reported it.
    pub usage: Option<Usage>,
}

impl Llm {
    /// Construct an [`Llm`] over an explicit backend.
    ///
    /// Primarily used by tests to inject a fake; production code should prefer
    /// [`Llm::from_env`].
    #[must_use]
    pub fn new(backend: Box<dyn StructuredBackend>) -> Self {
        Self { backend }
    }

    /// Construct an [`Llm`] targeting `model` (a `"provider/model"` routing
    /// string), reading the provider API key from the environment.
    ///
    /// # Errors
    ///
    /// Returns [`LlmError::EmptyModel`] if `model` is empty or whitespace.
    ///
    /// # Examples
    ///
    /// ```
    /// use monomyth_llm::Llm;
    ///
    /// let llm = Llm::from_env("anthropic/claude-sonnet-4-20250514")?;
    /// assert!(Llm::from_env("").is_err());
    /// # Ok::<(), monomyth_llm::LlmError>(())
    /// ```
    pub fn from_env(model: &str) -> Result<Self, LlmError> {
        if model.trim().is_empty() {
            return Err(LlmError::EmptyModel);
        }
        Ok(Self::new(Box::new(XbergBackend::new(model))))
    }

    /// Generate a value of type `T`, constraining the model to `T`'s JSON schema.
    ///
    /// On a deserialization failure the prompt is augmented with the parser error
    /// and retried up to [`MAX_RETRIES`] times. Token usage is accumulated across
    /// every attempt, so [`Generated::usage`] (and [`LlmError::Parse`]'s `usage`)
    /// reflect the full cost of the call, not just the final round-trip.
    ///
    /// # Errors
    ///
    /// - [`LlmError::Schema`] if `T`'s schema cannot be serialized.
    /// - [`LlmError::Backend`] if a backend call fails. A backend error on *any*
    ///   attempt terminates the retry loop immediately; only deserialization
    ///   failures trigger a retry.
    /// - [`LlmError::Parse`] if no attempt produced valid output.
    pub async fn generate<T>(
        &self,
        prompt: &str,
        schema_name: &str,
    ) -> Result<Generated<T>, LlmError>
    where
        T: DeserializeOwned + JsonSchema,
    {
        let schema = serde_json::to_value(schema_for!(T))?;
        let mut current_prompt = prompt.to_owned();
        let mut last_error = String::new();
        let mut total_usage: Option<Usage> = None;
        let mut attempts_made = 0;

        for attempt in 1..=MAX_ATTEMPTS {
            attempts_made = attempt;
            let (value, usage) = self
                .backend
                .complete_json(&current_prompt, schema_name, &schema)
                .await
                .map_err(|source| LlmError::backend(format!("generate '{schema_name}'"), source))?;

            total_usage = accumulate_usage(total_usage, usage);

            match serde_json::from_value::<T>(value) {
                Ok(value) => {
                    return Ok(Generated {
                        value,
                        usage: total_usage,
                    });
                }
                Err(error) => {
                    last_error = error.to_string();
                    if attempt < MAX_ATTEMPTS {
                        current_prompt = augment_prompt(prompt, &last_error);
                    }
                }
            }
        }

        Err(LlmError::Parse {
            attempts: attempts_made,
            last_error,
            usage: total_usage,
        })
    }

    /// Complete `prompt` as free-form text, returning the text alongside usage
    /// metadata (symmetric with [`Llm::generate`]).
    ///
    /// # Errors
    ///
    /// Returns [`LlmError::Backend`] if the backend call fails.
    pub async fn text(&self, prompt: &str) -> Result<Generated<String>, LlmError> {
        let (value, usage) = self
            .backend
            .complete_text(prompt)
            .await
            .map_err(|source| LlmError::backend("text completion", source))?;
        Ok(Generated { value, usage })
    }
}

/// Fold a call's usage into the running total, treating a missing running total
/// or a missing per-call usage as "nothing to add" rather than zero.
fn accumulate_usage(total: Option<Usage>, next: Option<Usage>) -> Option<Usage> {
    match (total, next) {
        (Some(total), Some(next)) => Some(total.saturating_add(&next)),
        (existing @ Some(_), None) | (None, existing @ Some(_)) => existing,
        (None, None) => None,
    }
}

/// Rebuild the prompt with a correction note describing the previous failure so
/// the model can repair its output on the next attempt.
fn augment_prompt(original: &str, last_error: &str) -> String {
    format!(
        "{original}\n\nYour previous response could not be parsed into the required schema. \
         Error: {last_error}\nReturn only valid JSON that conforms to the schema."
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use schemars::JsonSchema;
    use serde::Deserialize;
    use serde_json::{Value, json};

    use super::{Generated, Llm, MAX_ATTEMPTS};
    use crate::backend::{StructuredBackend, Usage};
    use crate::error::{BackendError, LlmError};

    #[derive(Debug, Deserialize, JsonSchema, PartialEq, Eq)]
    struct Hero {
        name: String,
        level: u32,
    }

    /// A scripted backend: `complete_json` pops the next canned response per call
    /// and records the prompt it was called with, so tests can assert on what the
    /// retry loop actually sends.
    type PromptLog = Arc<Mutex<Vec<String>>>;

    struct FakeBackend {
        json_responses: Mutex<Vec<Value>>,
        seen_prompts: PromptLog,
        text_response: String,
        usage: Option<Usage>,
    }

    impl FakeBackend {
        fn with_json(responses: Vec<Value>, usage: Option<Usage>) -> Self {
            Self {
                json_responses: Mutex::new(responses),
                seen_prompts: PromptLog::default(),
                text_response: String::new(),
                usage,
            }
        }

        fn with_text(text: &str) -> Self {
            Self {
                json_responses: Mutex::new(Vec::new()),
                seen_prompts: PromptLog::default(),
                text_response: text.to_owned(),
                usage: None,
            }
        }

        /// A shared handle to the recorded prompt log, cloned out before the
        /// backend is moved into an [`Llm`].
        fn prompt_log(&self) -> PromptLog {
            Arc::clone(&self.seen_prompts)
        }
    }

    #[async_trait]
    impl StructuredBackend for FakeBackend {
        async fn complete_json(
            &self,
            prompt: &str,
            _schema_name: &str,
            _schema: &Value,
        ) -> Result<(Value, Option<Usage>), BackendError> {
            self.seen_prompts
                .lock()
                .expect("lock poisoned")
                .push(prompt.to_owned());
            let mut responses = self.json_responses.lock().expect("lock poisoned");
            if responses.is_empty() {
                return Err(BackendError::new("no scripted responses remain"));
            }
            Ok((responses.remove(0), self.usage.clone()))
        }

        async fn complete_text(
            &self,
            _prompt: &str,
        ) -> Result<(String, Option<Usage>), BackendError> {
            Ok((self.text_response.clone(), None))
        }
    }

    fn llm_with(backend: FakeBackend) -> Llm {
        Llm::new(Box::new(backend))
    }

    #[tokio::test]
    async fn should_deserialize_and_map_usage_on_happy_path() {
        let usage = Usage {
            prompt_tokens: Some(11),
            completion_tokens: Some(7),
            total_tokens: Some(18),
        };
        let backend = FakeBackend::with_json(
            vec![json!({ "name": "Gilgamesh", "level": 5 })],
            Some(usage.clone()),
        );

        let Generated {
            value,
            usage: reported,
        } = llm_with(backend)
            .generate::<Hero>("forge a hero", "hero")
            .await
            .expect("generation should succeed");

        assert_eq!(
            value,
            Hero {
                name: "Gilgamesh".to_owned(),
                level: 5
            },
            "value must equal the canned hero exactly"
        );
        assert_eq!(
            reported,
            Some(usage),
            "usage must be passed through unchanged"
        );
    }

    #[tokio::test]
    async fn should_recover_when_first_response_is_malformed() {
        let backend = FakeBackend::with_json(
            vec![
                json!({ "name": "Enkidu" }),
                json!({ "name": "Enkidu", "level": 3 }),
            ],
            None,
        );

        let Generated { value, .. } = llm_with(backend)
            .generate::<Hero>("forge a hero", "hero")
            .await
            .expect("second attempt should recover");

        assert_eq!(
            value,
            Hero {
                name: "Enkidu".to_owned(),
                level: 3
            },
            "must return the second, valid response"
        );
    }

    #[tokio::test]
    async fn should_augment_retry_prompt_from_the_original_with_the_parse_error() {
        const ORIGINAL: &str = "forge a hero";
        let backend = FakeBackend::with_json(
            vec![
                json!({ "name": "Enkidu" }),
                json!({ "name": "Enkidu", "level": 3 }),
            ],
            None,
        );
        let prompts = backend.prompt_log();

        let _recovered = llm_with(backend)
            .generate::<Hero>(ORIGINAL, "hero")
            .await
            .expect("second attempt should recover");

        let sent = prompts.lock().expect("lock poisoned").clone();
        assert_eq!(
            sent.len(),
            2,
            "exactly two backend calls should have occurred"
        );
        assert_eq!(
            sent[0], ORIGINAL,
            "the first call must send the original prompt verbatim"
        );
        assert!(
            sent[1].starts_with(ORIGINAL),
            "the retry prompt must be built from the original, not a compounded prompt",
        );
        assert!(
            sent[1].contains("missing field `level`"),
            "the retry prompt must feed the parse error back to the model, got: {}",
            sent[1],
        );
    }

    #[tokio::test]
    async fn should_error_with_exhausted_attempts_when_always_malformed() {
        let malformed = vec![
            json!({ "name": "x" }),
            json!({ "name": "y" }),
            json!({ "name": "z" }),
        ];
        let per_call = Usage {
            prompt_tokens: Some(4),
            completion_tokens: Some(2),
            total_tokens: Some(6),
        };
        let backend = FakeBackend::with_json(malformed, Some(per_call));

        let error = llm_with(backend)
            .generate::<Hero>("forge a hero", "hero")
            .await
            .expect_err("generation must fail when every response is malformed");

        match error {
            LlmError::Parse {
                attempts, usage, ..
            } => {
                assert_eq!(
                    attempts, MAX_ATTEMPTS,
                    "attempts must equal MAX_ATTEMPTS (MAX_RETRIES + 1)"
                );
                assert_eq!(attempts, 3, "documented total attempt count is 3");
                assert_eq!(
                    usage,
                    Some(Usage {
                        prompt_tokens: Some(12),
                        completion_tokens: Some(6),
                        total_tokens: Some(18),
                    }),
                    "a failed call still costs the caller: usage sums over all 3 attempts",
                );
            }
            other => panic!("expected LlmError::Parse, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_accumulate_usage_across_a_retry() {
        let per_call = Usage {
            prompt_tokens: Some(5),
            completion_tokens: Some(3),
            total_tokens: Some(8),
        };
        let backend = FakeBackend::with_json(
            vec![
                json!({ "name": "Enkidu" }),
                json!({ "name": "Enkidu", "level": 3 }),
            ],
            Some(per_call),
        );

        let Generated { usage, .. } = llm_with(backend)
            .generate::<Hero>("forge a hero", "hero")
            .await
            .expect("second attempt should recover");

        assert_eq!(
            usage,
            Some(Usage {
                prompt_tokens: Some(10),
                completion_tokens: Some(6),
                total_tokens: Some(16),
            }),
            "reported usage must sum both round-trips, not just the successful one",
        );
    }

    #[tokio::test]
    async fn should_wrap_backend_error_with_operation_context() {
        let backend = FakeBackend::with_json(Vec::new(), None);

        let error = llm_with(backend)
            .generate::<Hero>("forge a hero", "hero")
            .await
            .expect_err("a backend failure must surface");

        match error {
            LlmError::Backend { context, source } => {
                assert_eq!(
                    context, "generate 'hero'",
                    "context must name the operation and schema",
                );
                assert_eq!(source.to_string(), "no scripted responses remain");
            }
            other => panic!("expected LlmError::Backend, got {other:?}"),
        }
    }

    #[test]
    fn should_reject_a_whitespace_only_model() {
        assert!(
            matches!(Llm::from_env("   "), Err(LlmError::EmptyModel)),
            "a whitespace-only model string must be rejected as empty",
        );
    }

    #[tokio::test]
    async fn should_pass_through_text_completion() {
        let backend = FakeBackend::with_text("the road of trials");

        let Generated { value, .. } = llm_with(backend)
            .text("describe the trial")
            .await
            .expect("text completion should succeed");

        assert_eq!(
            value, "the road of trials",
            "text must be returned verbatim"
        );
    }
}
