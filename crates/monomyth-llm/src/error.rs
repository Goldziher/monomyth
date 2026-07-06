//! Error types for the typed LLM wrapper.
//!
//! [`BackendError`] is the opaque error a [`crate::StructuredBackend`] returns,
//! keeping xberg's concrete error type out of the public trait. [`LlmError`] is
//! the crate's public error surface.

use thiserror::Error;

/// An opaque error returned by a [`crate::StructuredBackend`].
///
/// The real backend maps xberg's error type into this so downstream code never
/// depends on xberg's error enum. It carries only a human-readable message.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct BackendError {
    message: String,
}

impl BackendError {
    /// Construct a [`BackendError`] from any displayable value.
    ///
    /// # Examples
    ///
    /// ```
    /// use monomyth_llm::BackendError;
    ///
    /// let error = BackendError::new("request timed out");
    /// assert_eq!(error.to_string(), "request timed out");
    /// ```
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// The public error type for typed LLM generation.
#[derive(Debug, Error)]
pub enum LlmError {
    /// [`crate::Llm::from_env`] was given an empty model string.
    #[error("model string must not be empty")]
    EmptyModel,

    /// The underlying backend call failed. Carries the operation context.
    #[error("backend call failed ({context}): {source}")]
    Backend {
        /// A short description of the operation that failed.
        context: String,
        /// The originating backend error.
        source: BackendError,
    },

    /// The generated JSON schema could not be serialized.
    #[error("schema serialization failed: {0}")]
    Schema(#[from] serde_json::Error),

    /// The model never produced JSON that deserialized into the target type.
    #[error("failed to parse model output after {attempts} attempt(s): {last_error}")]
    Parse {
        /// The total number of attempts made before giving up.
        attempts: u32,
        /// The final deserialization error message.
        last_error: String,
    },
}

impl LlmError {
    /// Wrap a [`BackendError`] with operation context.
    pub(crate) fn backend(context: impl Into<String>, source: BackendError) -> Self {
        Self::Backend {
            context: context.into(),
            source,
        }
    }
}
