//! The build-time law-drafting pipeline (ADR-0016 Phase 2b/2c).
//!
//! `monomyth-synthesis` turns REFERENCE-namespace corpus material into a
//! pre-review, ship-safe candidate law artifact, without ever surfacing
//! reference prose. The pipeline:
//!
//! 1. **Retrieve** reference-namespace passages via `monomyth-knowledge`
//!    ([`monomyth_knowledge::KnowledgeQuery::reference`]) — priors only, never
//!    surfaced.
//! 2. **Distill**: ask the LLM to produce an abstract structural taxonomy (an
//!    uncopyrightable idea — [`monomyth_frameworks`]'s `Tier::System`,
//!    ship-safe) from those passages. The model's authority is the *idea*
//!    only — a title and a list of named, one-sentence-described items
//!    ([`CandidateLaw`]) — never wording, never ids, never licensing metadata.
//! 3. **Gate**: run the machine-checked anti-leak check
//!    ([`verify_no_verbatim`]) over every candidate text field against every
//!    retrieved passage. This is the 4th enforcement layer atop ADR-0005's
//!    three (ingest gate, retrieval gate, CI audit): even a distillation-only
//!    prompt can accidentally reproduce source wording, so a machine check
//!    backstops the prompt's own instructions before anything is emitted.
//! 4. **Stamp**: produce a pre-review [`LawArtifact`]
//!    ([`monomyth_frameworks::LawArtifact`]) with `synthesis.reviewed_by` left
//!    empty. [`monomyth_frameworks::load_law`] refuses to load an artifact
//!    with an empty reviewer, so a drafted candidate is structurally incapable
//!    of shipping until a human reviews it, edits wording as needed, and
//!    stamps a reviewer.
//!
//! A human review step (outside this crate) is what promotes a
//! [`DraftedLaw::artifact`] into a committed `artifacts/laws/*.json` file.

#![forbid(unsafe_code)]

mod antileak;
mod candidate;
mod error;
mod pipeline;

pub use antileak::verify_no_verbatim;
pub use candidate::{CandidateItem, CandidateLaw};
pub use error::SynthesisError;
pub use pipeline::{DraftRequest, DraftedLaw, draft_law};

// Re-exported so a caller building a `DraftRequest`/`draft_law` call site does
// not need a direct `monomyth-frameworks` dependency just to name the
// artifact type its result carries.
pub use monomyth_frameworks::LawArtifact;
