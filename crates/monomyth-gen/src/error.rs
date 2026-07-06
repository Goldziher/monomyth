//! The error type for generation.
//!
//! Procedural generation is a pure function of a seed and rarely fails: the only
//! failures are contract violations (an empty pipeline, or a pass run out of the
//! order its inputs require). The content phase adds the fallible variants that
//! reach out of the process — an [`Llm`](GenError::Llm) call and a
//! [`Knowledge`](GenError::Knowledge) retrieval. The enum is
//! [`non_exhaustive`](macro@non_exhaustive) so further variants can be added
//! without a breaking change.

use thiserror::Error;

/// Errors that can arise while assembling a world from a seed.
#[non_exhaustive]
#[derive(Debug, Error)]
pub enum GenError {
    /// A [`Generator`](crate::Generator) was run with no passes, so it would
    /// produce an empty, structureless world.
    #[error("the generator pipeline is empty; add at least one procedural pass")]
    EmptyPipeline,

    /// A pass that populates the world graph ran before any locations existed.
    ///
    /// The default pipeline runs [`MapPass`](crate::MapPass) first for exactly
    /// this reason; a custom pipeline that reorders it triggers this error.
    #[error("pass `{pass}` ran before any locations existed; the map pass must run first")]
    NoLocations {
        /// The name of the pass that found an empty location graph.
        pass: &'static str,
    },

    /// An internal invariant the generator relies on was violated.
    ///
    /// This signals a bug in a pass rather than bad input; the message names the
    /// broken assumption.
    #[error("internal generator invariant violated: {0}")]
    Invariant(&'static str),

    /// An LLM call in the content phase failed (backend error, or model output
    /// that could not be parsed into the requested schema).
    #[error("content generation LLM call failed")]
    Llm(#[from] monomyth_llm::LlmError),

    /// A ship-safe grounding retrieval in the content phase failed.
    #[error("content grounding retrieval failed")]
    Knowledge(#[from] monomyth_knowledge::KnowledgeError),
}
