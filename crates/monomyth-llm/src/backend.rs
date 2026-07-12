//! The backend seam.
//!
//! xberg's structured-completion helper builds its own client internally and is
//! not injectable, so we define our own [`StructuredBackend`] trait. Tests inject
//! a fake; production uses [`XbergBackend`], which calls the xberg free functions
//! and maps their results into crate-local types.

use std::fmt;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use xberg::core::config::LlmConfig;
use xberg::types::LlmUsage;

use crate::error::BackendError;

/// The label passed to xberg as the usage `source` for every call this crate makes.
const USAGE_SOURCE: &str = "monomyth-llm";

/// Token accounting for a single LLM call, decoupled from xberg's `LlmUsage`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Input/prompt tokens consumed.
    pub prompt_tokens: Option<u64>,
    /// Output/completion tokens generated.
    pub completion_tokens: Option<u64>,
    /// Total tokens (input + output).
    pub total_tokens: Option<u64>,
}

impl Usage {
    /// Map an xberg [`LlmUsage`] into the crate-local [`Usage`], keeping xberg
    /// types internal to this crate (no public `From` impl on an xberg type).
    pub(crate) fn from_xberg(usage: &LlmUsage) -> Self {
        Self {
            prompt_tokens: usage.input_tokens,
            completion_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
        }
    }

    /// Sum two usage records field-by-field, saturating on overflow.
    ///
    /// Used to accumulate token cost across the retry attempts of a single
    /// [`generate`](crate::Llm::generate) call so the reported total reflects
    /// every provider round-trip, not just the last one. A present count added
    /// to an absent one carries the present value through.
    #[must_use]
    pub fn saturating_add(&self, other: &Self) -> Self {
        Self {
            prompt_tokens: add_optional(self.prompt_tokens, other.prompt_tokens),
            completion_tokens: add_optional(self.completion_tokens, other.completion_tokens),
            total_tokens: add_optional(self.total_tokens, other.total_tokens),
        }
    }
}

/// Sum two optional counts, treating `None` as "unreported" rather than zero:
/// the result is `Some` iff at least one operand is `Some`.
fn add_optional(left: Option<u64>, right: Option<u64>) -> Option<u64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.saturating_add(right)),
        (value @ Some(_), None) | (None, value @ Some(_)) => value,
        (None, None) => None,
    }
}

/// A structured-output LLM backend.
///
/// The seam that keeps generation testable: production talks to a real provider
/// via [`XbergBackend`], tests substitute a fake returning canned responses.
#[async_trait]
pub trait StructuredBackend: Send + Sync {
    /// Complete `prompt`, constraining the model to `schema`, and return the raw
    /// JSON value plus any usage metadata.
    ///
    /// # Errors
    ///
    /// Returns a [`BackendError`] if the request fails or the response cannot be
    /// obtained as JSON.
    async fn complete_json(
        &self,
        prompt: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError>;

    /// Complete `prompt` as free-form text and return the response plus any usage
    /// metadata.
    ///
    /// # Errors
    ///
    /// Returns a [`BackendError`] if the request fails or returns no content.
    async fn complete_text(&self, prompt: &str) -> Result<(String, Option<Usage>), BackendError>;
}

/// Tunable transport knobs applied to the underlying provider client.
///
/// These map onto xberg's [`LlmConfig`] without exposing it in this crate's
/// public API. `max_retries` bounds xberg's own transport-retry layer, which
/// composes *multiplicatively* with this crate's JSON-repair loop
/// ([`MAX_ATTEMPTS`](crate::MAX_ATTEMPTS) in `llm.rs`): a worst-case call makes
/// up to `MAX_ATTEMPTS` parse attempts, each of which may retry the transport up
/// to `max_retries` times, for a bounded worst case of 3 parse attempts ×
/// up-to-3 transport tries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BackendOptions {
    /// Request timeout, in seconds, applied to each transport call.
    pub timeout_secs: Option<u64>,
    /// Maximum tokens the provider is allowed to generate per call.
    pub max_tokens: Option<u64>,
    /// Sampling temperature passed to the provider.
    pub temperature: Option<f64>,
    /// Maximum transport-level retry attempts xberg grants per call.
    pub max_retries: Option<u32>,
}

impl Default for BackendOptions {
    /// A hardened default: a 60-second timeout and up to 2 transport retries,
    /// with no explicit token cap or temperature override (the provider's own
    /// defaults apply).
    fn default() -> Self {
        Self {
            timeout_secs: Some(60),
            max_tokens: None,
            temperature: None,
            max_retries: Some(2),
        }
    }
}

/// The production [`StructuredBackend`], backed by xberg's LLM helpers.
#[derive(Clone)]
pub struct XbergBackend {
    config: LlmConfig,
}

impl fmt::Debug for XbergBackend {
    /// Deliberately opaque: `LlmConfig` may carry provider secrets (an API key),
    /// so its fields must never reach a log or panic message via `Debug`.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("XbergBackend")
            .finish_non_exhaustive()
    }
}

impl XbergBackend {
    /// Construct a backend targeting `model` — a `"provider/model"` routing
    /// string (e.g. `"anthropic/claude-sonnet-4-20250514"`). The provider API
    /// key is read from the environment at call time. Transport knobs are set
    /// from [`BackendOptions::default`] (a 60s timeout, 2 max retries).
    ///
    /// The xberg [`LlmConfig`] is built internally and never appears in this
    /// crate's public API.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self::with_options(model, BackendOptions::default())
    }

    /// Construct a backend targeting `model` with explicit transport tuning.
    ///
    /// The provider API key is read from the environment at call time. The
    /// xberg [`LlmConfig`] is built internally and never appears in this
    /// crate's public API.
    #[must_use]
    pub fn with_options(model: impl Into<String>, options: BackendOptions) -> Self {
        let model = model.into();
        let api_key = provider_api_key(&model);
        Self {
            config: LlmConfig {
                model,
                api_key,
                timeout_secs: options.timeout_secs,
                max_retries: options.max_retries,
                temperature: options.temperature,
                max_tokens: options.max_tokens,
                ..LlmConfig::default()
            },
        }
    }
}

/// Resolve a provider's API key from its standard environment variable, given a
/// `"provider/model"` routing string.
///
/// xberg hands liter-llm `config.api_key.unwrap_or_default()` — an *empty*
/// string when the key is `None` — and liter-llm's Google provider forwards that
/// verbatim as the `x-goog-api-key` header rather than falling back to the
/// environment, so an unset key yields a `403 PERMISSION_DENIED` instead of an
/// env lookup. We therefore resolve the key explicitly here and set it on the
/// config. Returns `None` for an unrecognised provider or an unset/empty
/// variable, preserving the previous "let the downstream default apply"
/// behaviour. The key only ever lives inside [`LlmConfig`], whose [`Debug`] is
/// opaque, so it never reaches a log.
fn provider_api_key(model: &str) -> Option<String> {
    let variable = if model.starts_with("openai/") {
        "OPENAI_API_KEY"
    } else if model.starts_with("anthropic/") {
        "ANTHROPIC_API_KEY"
    } else if model.starts_with("gemini/") {
        "GEMINI_API_KEY"
    } else if model.starts_with("mistral/") {
        "MISTRAL_API_KEY"
    } else {
        return None;
    };
    std::env::var(variable)
        .ok()
        .filter(|key| !key.trim().is_empty())
}

/// Root-level JSON Schema draft metadata keywords that carry no validation
/// semantics but which some providers reject outright. `schemars` emits
/// `$schema` and `title` at the schema root; Google's Gemini `responseSchema`
/// validator errors on the first such unknown field (`Unknown name "$schema"`),
/// so the request never reaches the model.
const UNSUPPORTED_ROOT_SCHEMA_KEYS: [&str; 3] = ["$schema", "$id", "title"];

/// Strip [`UNSUPPORTED_ROOT_SCHEMA_KEYS`] from the schema *root only*, unless the
/// model is an `OpenAI` one (whose strict mode tolerates them, and whose path we
/// keep byte-identical — mirroring xberg's own `additionalProperties` gating).
///
/// Stripping only at the root is deliberate: these are schema keywords when they
/// are direct keys of the schema object, but a user field literally named
/// `title` would appear as a key *under* `properties`, where it must be
/// preserved. A recursive strip would corrupt such a schema.
fn sanitize_schema_for_model(model: &str, schema: &Value) -> Value {
    if model.starts_with("openai/") {
        return schema.clone();
    }
    let Value::Object(map) = schema else {
        return schema.clone();
    };
    let mut cleaned = map.clone();
    for key in UNSUPPORTED_ROOT_SCHEMA_KEYS {
        cleaned.remove(key);
    }
    Value::Object(cleaned)
}

#[async_trait]
impl StructuredBackend for XbergBackend {
    async fn complete_json(
        &self,
        prompt: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError> {
        let sanitized = sanitize_schema_for_model(&self.config.model, schema);
        let (value, usage) = xberg::llm::structured::complete_with_json_schema(
            &self.config,
            prompt,
            schema_name,
            &sanitized,
            USAGE_SOURCE,
        )
        .await
        .map_err(|error| BackendError::new(error.to_string()))?;
        Ok((value, usage.as_ref().map(Usage::from_xberg)))
    }

    async fn complete_text(&self, prompt: &str) -> Result<(String, Option<Usage>), BackendError> {
        let (text, usage) =
            xberg::llm::text_completion::complete_text(&self.config, prompt, USAGE_SOURCE)
                .await
                .map_err(|error| BackendError::new(error.to_string()))?;
        Ok((text, usage.as_ref().map(Usage::from_xberg)))
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::sanitize_schema_for_model;

    /// The shape `schemars` produces for a flat DTO: `$schema`/`title` at the
    /// root, with a `title`-named *property* that must survive sanitization.
    fn schemars_like() -> serde_json::Value {
        json!({
            "$schema": "https://json-schema.org/draft/2020-12/schema",
            "$id": "urn:example",
            "title": "NamedProse",
            "type": "object",
            "properties": {
                "title": { "type": "string" },
                "description": { "type": "string" }
            },
            "required": ["title", "description"]
        })
    }

    #[test]
    fn strips_root_metadata_for_gemini_but_keeps_a_title_property() {
        let cleaned = sanitize_schema_for_model("gemini/gemini-3.5-flash", &schemars_like());
        let object = cleaned.as_object().expect("schema stays an object");

        assert!(!object.contains_key("$schema"), "root $schema is stripped");
        assert!(!object.contains_key("$id"), "root $id is stripped");
        assert!(
            !object.contains_key("title"),
            "root title keyword is stripped"
        );
        assert_eq!(object["type"], json!("object"), "structural keys survive");
        assert!(
            object["properties"].get("title").is_some(),
            "a property literally named `title` must be preserved",
        );
        assert_eq!(object["required"], json!(["title", "description"]));
    }

    #[test]
    fn leaves_openai_schemas_untouched() {
        let original = schemars_like();
        let cleaned = sanitize_schema_for_model("openai/gpt-4o-mini", &original);
        assert_eq!(cleaned, original, "the OpenAI path is byte-identical");
    }
}
