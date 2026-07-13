//! The error type for the compose pipeline.
//!
//! Plan's sole failure mode is a [`World`](monomyth_core::World) with no
//! composable spine. The multi-turn long-form generation primitive
//! ([`crate::generate_long_form`]) adds two more: a caller misuse (asking for
//! zero turns) and a wrapped [`LlmError`] from the underlying model call.
//! Draft's per-section reference-grounding retrieval adds a wrapped
//! [`KnowledgeError`]. The revise-loop variant lands with a later slice, once
//! that phase exists to fail — it is deliberately not added here.

use monomyth_knowledge::KnowledgeError;
use monomyth_llm::LlmError;
use thiserror::Error;

/// Errors that can arise while composing long-form text from a [`World`](monomyth_core::World).
///
/// Not `Copy`/`Eq`: [`ComposeError::Generation`] wraps [`LlmError`], which is
/// neither (it carries a [`monomyth_llm::BackendError`] and, on a parse
/// failure, token usage — none of which are `Copy`, and a `BackendError`'s
/// message-only equality would be a poor proxy for error identity, so `Eq` is
/// dropped too). [`ComposeError::Retrieval`] wraps [`KnowledgeError`], which is
/// likewise neither.
#[derive(Debug, Error)]
pub enum ComposeError {
    /// The world's narrative spine has no nodes, so there is nothing to outline.
    #[error("the world's narrative spine is empty; nothing to compose")]
    EmptyOutline,

    /// [`crate::generate_long_form`] was called with `max_turns == 0`, which can
    /// never produce content.
    #[error("generate_long_form requires at least one turn (max_turns == 0)")]
    NoTurns,

    /// The underlying LLM call failed while generating long-form prose.
    #[error("generating long-form prose: {0}")]
    Generation(#[from] LlmError),

    /// Retrieving reference-path grounding for a section failed.
    #[error("retrieving reference grounding for a section: {0}")]
    Retrieval(#[from] KnowledgeError),
}
