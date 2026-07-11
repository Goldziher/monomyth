//! Cross-plane seam traits for the monomyth engine (ADR-0013, trait-first).
//!
//! These traits are the *contracts* code depends on across a plane boundary:
//! generation, extraction, and the knowledge layer each live behind a `dyn`/
//! `impl` seam declared here rather than depending on one another's concrete
//! types. Keeping the seams in their own crate (not in `monomyth-core`) preserves
//! `monomyth-core`'s no-IO/no-async invariant while still giving every plane one
//! shared vocabulary to program against.
//!
//! # What lives here
//!
//! - [`PassageRetriever`] — the P-KNOWLEDGE retrieval seam: turn a query into
//!   scored hits, without a caller depending on `monomyth-knowledge` or its
//!   vector-store backend.
//! - [`Classifier`] — classify a fragment of narrative text into a scored
//!   distribution over one framework axis (e.g. a Campbell stage).
//! - [`Extractor`] — the P-TRANSFORM inverse of generation (ADR-0018): derive a
//!   valid [`World`](monomyth_core::World) from input.
//!
//! Each trait is the seam its ADR calls for; concrete implementations (the
//! RAG-softmax classifier, the structure-preserving extractor) live in
//! `monomyth-extract`, and the retrieval adapter over `monomyth-knowledge` lives
//! with its consumer, never here — this crate stays free of every heavy backend.

use async_trait::async_trait;

use monomyth_core::{ScoredOne, World};

/// One scored retrieval hit: the relevance the backend assigned to a passage for
/// a query.
///
/// Deliberately minimal. A retrieved passage in `monomyth-knowledge` carries full
/// licensing provenance (source id, license, namespace, URL, checksum), but a
/// classifier only needs each hit's *relevance*, so the seam surfaces just that
/// and stays independent of the concrete `Passage` type. Provenance-aware callers
/// (e.g. the fine-tune export's public-domain gate) consult the license ledger
/// and the fixture, not this seam.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScoredHit {
    /// The backend's relevance score for this hit; higher is more relevant.
    pub score: f32,
}

/// A retrieval failure surfaced across the seam.
///
/// The concrete error (a `monomyth-knowledge` store failure, say) is flattened to
/// a message so this crate need not depend on any backend's error enum; the
/// adapter that wraps a concrete retriever is responsible for the conversion.
#[derive(Debug, thiserror::Error)]
#[error("passage retrieval failed: {0}")]
pub struct RetrievalError(pub String);

/// The retrieval seam: score a query against a corpus.
///
/// Implemented by an adapter over `monomyth-knowledge`'s ship-gated retrieval in
/// production, and by a deterministic fake in tests — so a [`Classifier`] can be
/// exercised hermetically without a real vector store, embedder, or network.
#[async_trait]
pub trait PassageRetriever: Send + Sync {
    /// Retrieve up to `top_k` scored hits for `query`, best-first is *not*
    /// promised — callers must not assume ordering and should aggregate over the
    /// returned set.
    ///
    /// # Errors
    ///
    /// [`RetrievalError`] if the underlying corpus/store cannot be queried.
    async fn retrieve_scores(
        &self,
        query: &str,
        top_k: u32,
    ) -> Result<Vec<ScoredHit>, RetrievalError>;
}

/// A classification failure.
#[derive(Debug, thiserror::Error)]
pub enum ClassifyError {
    /// Retrieval failed while gathering evidence for the classification.
    #[error("retrieval failed during classification: {0}")]
    Retrieval(#[from] RetrievalError),
    /// No candidate in the vocabulary gathered any evidence, so the resulting
    /// distribution would be undefined (a division by zero) rather than merely
    /// flat. A caller should treat this as "cannot classify", not "classified as
    /// uniform".
    #[error("no candidate gathered any evidence; classification is undefined")]
    NoEvidence,
}

/// Classify a fragment of narrative text into a scored distribution over one
/// framework axis `T` (a Campbell stage, a Propp function, …).
///
/// The result is a [`ScoredOne`]: a single most-likely label plus weighted
/// runners-up, matching how the scored schema (ADR-0022) records a mandatory
/// single-valued axis. Converting the classifier's internal distribution to
/// `ScoredOne` (primary = argmax, alternatives = the rest as permille weights) is
/// the implementation's responsibility.
#[async_trait]
pub trait Classifier<T>: Send + Sync
where
    T: Ord + Send + 'static,
{
    /// Classify `text`, returning the scored distribution over `T`.
    ///
    /// # Errors
    ///
    /// [`ClassifyError`] if evidence gathering fails or yields nothing to rank.
    async fn classify(&self, text: &str) -> Result<ScoredOne<T>, ClassifyError>;
}

/// An extraction failure.
#[derive(Debug, thiserror::Error)]
pub enum ExtractError {
    /// Classifying one of the input's beats failed.
    #[error("classification failed during extraction: {0}")]
    Classify(#[from] ClassifyError),
    /// The classified labels could not be written back as a valid world (an edit
    /// that would have broken a structural invariant).
    #[error("applying extracted edits to the world failed: {0}")]
    Edit(#[from] monomyth_core::EditError),
}

/// Derive a valid [`World`] from input — the inverse of generation (ADR-0018).
///
/// # Proof-of-concept scope
///
/// The first implementation (`monomyth-extract`'s structure-preserving
/// re-classifier) takes a skeleton [`World`] whose narrative *structure* is given
/// and re-derives its scored classifications, so extraction can be measured
/// against a gold fixture on the classification axes alone. ADR-0018's fuller
/// signature — extracting structure *and* classification from raw source text,
/// parameterized by config and genre — is future work layered on this seam once
/// those inputs exist; the seam is intentionally small until then.
#[async_trait]
pub trait Extractor: Send + Sync {
    /// Extract a classified [`World`] from `skeleton`.
    ///
    /// # Errors
    ///
    /// [`ExtractError`] if classification fails or the result is not a valid
    /// world.
    async fn extract(&self, skeleton: &World) -> Result<World, ExtractError>;
}
