//! The error type for the compose pipeline.
//!
//! This slice only plans: the sole failure mode is a [`World`](monomyth_core::World)
//! with no composable spine. Generation and retrieval variants land with the
//! Draft/Revise/Assemble slices, once those phases exist to fail — they are
//! deliberately not added here.

use thiserror::Error;

/// Errors that can arise while composing long-form text from a [`World`](monomyth_core::World).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub enum ComposeError {
    /// The world's narrative spine has no nodes, so there is nothing to outline.
    #[error("the world's narrative spine is empty; nothing to compose")]
    EmptyOutline,
}
