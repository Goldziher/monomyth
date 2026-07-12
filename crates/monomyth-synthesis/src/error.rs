//! The error model for the build-time synthesis pipeline.

use monomyth_knowledge::KnowledgeError;
use monomyth_llm::LlmError;
use thiserror::Error;

/// Errors raised while drafting a candidate law artifact (ADR-0016 Phase 2b/2c).
///
/// [`non_exhaustive`](macro@non_exhaustive) so a future enforcement layer can
/// add a variant without breaking downstream matches.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SynthesisError {
    /// A reference-collection retrieval failed.
    #[error("reference retrieval failed")]
    Retrieval(#[from] KnowledgeError),

    /// An LLM call failed: backend error, or output that would not parse into
    /// the expected schema ([`crate::candidate::CandidateLaw`] for a
    /// distillation/refine call, [`crate::judge::JudgeVerdict`] for a judge
    /// call).
    #[error("law drafting LLM call failed")]
    Generation(#[from] LlmError),

    /// Retrieval returned zero reference passages for the query.
    ///
    /// A law synthesized with no grounding would be pure invention rather than
    /// a distillation of corpus material, so the pipeline refuses outright
    /// rather than asking the model to draft from nothing.
    #[error("reference query {query:?} returned no passages; refusing to synthesize ungrounded")]
    NoReferenceGrounding {
        /// The query text that returned no passages.
        query: String,
    },

    /// A reference-namespace query ([`monomyth_knowledge::KnowledgeQuery::reference`])
    /// returned a passage that reports itself surfaceable
    /// ([`monomyth_knowledge::Passage::is_surfaceable`]).
    ///
    /// This can only happen if the reference collection was incorrectly populated (a
    /// ship source ingested into it, or a stored namespace tag disagreeing with
    /// the ledger). It is a licensing-invariant breach — defense in depth atop
    /// `monomyth-knowledge`'s own ingest/retrieval gates — so the pipeline hard
    /// refuses rather than silently trusting the query path.
    #[error(
        "reference query returned a surfaceable passage from source '{source_id}'; refusing to \
         treat it as a prior (licensing-invariant breach)"
    )]
    SurfaceablePassageInReferenceQuery {
        /// The offending passage's declared source id.
        source_id: String,
    },

    /// The anti-leak gate ([`crate::antileak::verify_no_verbatim`]) found a
    /// candidate span that verbatim-overlaps a reference chunk.
    ///
    /// A tripped gate means the model's authority (the abstract idea) has
    /// bled into its wording (the source's actual prose); the candidate is
    /// discarded rather than emitted, even in pre-review form.
    #[error(
        "candidate text overlaps source '{source_id}' verbatim: {candidate_span:?}; refusing to \
         emit the candidate"
    )]
    VerbatimOverlap {
        /// The offending span, reconstructed from the shared shingle.
        candidate_span: String,
        /// The reference source the span was found to overlap.
        source_id: String,
    },
}
