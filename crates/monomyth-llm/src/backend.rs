//! The backend seam.
//!
//! xberg's structured-completion helper builds its own client internally and is
//! not injectable, so we define our own [`StructuredBackend`] trait. Tests inject
//! a fake; production uses [`XbergBackend`], which calls the xberg free functions
//! and maps their results into crate-local types.

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

/// The production [`StructuredBackend`], backed by xberg's LLM helpers.
#[derive(Debug, Clone)]
pub struct XbergBackend {
    config: LlmConfig,
}

impl XbergBackend {
    /// Construct a backend targeting `model` — a `"provider/model"` routing
    /// string (e.g. `"anthropic/claude-sonnet-4-20250514"`). The provider API
    /// key is read from the environment at call time.
    ///
    /// The xberg [`LlmConfig`] is built internally and never appears in this
    /// crate's public API.
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            config: LlmConfig {
                model: model.into(),
                ..LlmConfig::default()
            },
        }
    }
}

#[async_trait]
impl StructuredBackend for XbergBackend {
    async fn complete_json(
        &self,
        prompt: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError> {
        let (value, usage) = xberg::llm::structured::complete_with_json_schema(
            &self.config,
            prompt,
            schema_name,
            schema,
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
