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
//! - [`StructureExtractor`] — the fuller ADR-0018 signature [`Extractor`]'s doc
//!   flags as future work: derive a valid [`World`] directly from raw source
//!   text, rather than only re-classifying an already-structured skeleton.
//! - [`Renderer`] — the P-RENDER seam (ADR-0020): turn a [`World`] into
//!   medium-specific output, without a caller depending on any one frontend crate.
//!
//! Each trait is the seam its ADR calls for; concrete implementations (the
//! RAG-softmax classifier, the structure-preserving extractor, the prose and
//! terse renderers) live in their own crates (`monomyth-extract`,
//! `monomyth-text`, `monomyth-render-terse`, …), and the retrieval adapter over
//! `monomyth-knowledge` lives with its consumer, never here — this crate stays
//! free of every heavy backend.

use async_trait::async_trait;

use monomyth_core::{Event, ScoredOne, World};

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

/// A [`StructureExtractor`] failure.
///
/// The generation-backend failure is flattened to a message rather than named
/// as a variant, mirroring [`RetrievalError`]: this crate stays free of
/// `monomyth-llm` (or any other heavy backend) so the seam alone can be
/// depended on cheaply, and an implementation converts its own backend error
/// with [`ToString`].
#[derive(Debug, thiserror::Error)]
pub enum StructureExtractError {
    /// The generation backend failed to produce a structured beat.
    #[error("structure generation failed: {0}")]
    Generation(String),
    /// The one-time structural bootstrap (creating the graph's first node) failed
    /// on a local precondition. No legal input should ever trigger this — see
    /// [`StructureExtractor`]'s doc for why the bootstrap step exists outside the
    /// edit vocabulary — but it is threaded through rather than unwrapped.
    #[error("world bootstrap failed: {0}")]
    Bootstrap(String),
    /// The model named a framework label (a Campbell stage id, …) absent from
    /// `monomyth-frameworks`' vocabulary.
    #[error("unrecognized framework label: {0}")]
    UnrecognizedLabel(String),
    /// The assembled world failed [`World::validate`] — the write surface's
    /// final gate, shared with generation's own output.
    #[error("the extracted world failed validation: {0}")]
    Invalid(#[from] monomyth_core::WorldError),
}

/// Derive a valid [`World`] directly from raw source text — the fuller
/// ADR-0018 signature that [`Extractor`]'s doc flags as future work.
///
/// # Why a new trait rather than widening `Extractor::extract`
///
/// [`Extractor::extract`] takes an already-*structured* skeleton `World` and
/// only re-derives its scored classifications (the classify-only proof of
/// concept, confirmed in ADR-0018). This seam's input/output shape is
/// genuinely different: it starts from nothing but `text` and must derive the
/// narrative *shape* itself (at minimum, one node and one location), not just
/// relabel an existing one. Widening `Extractor::extract`'s signature to take
/// `Option<&World>` or similar would force `StageReclassifyingExtractor` (its
/// only existing implementer) to handle an input shape it was never designed
/// for, and would blur two independently-testable proof-of-concept scopes into
/// one trait. A new trait keeps both seams small, keeps
/// `StageReclassifyingExtractor` undisturbed, and matches how [`Extractor`]
/// and [`StructureExtractor`] are peer transforms over the same pivot, not one
/// a special case of the other.
///
/// # Write surface
///
/// An implementation must build the result exclusively through
/// [`NarrativeEdit`](monomyth_core::NarrativeEdit) for the node's *extracted
/// content* (label, stage, synopsis hint, …) and gate the assembled world with
/// [`World::validate`] — the same write surface and gate generation uses. The
/// one exception, applying equally to every bootstrapper in this workspace
/// (including generation's own `BackbonePass`), is naming the very first node
/// as the structure's root: no `NarrativeEdit` variant can express that step,
/// because there is nothing pre-existing for an edit to apply to.
#[async_trait]
pub trait StructureExtractor: Send + Sync {
    /// Derive a valid [`World`] from `text`.
    ///
    /// # Errors
    ///
    /// [`StructureExtractError`] if generation fails, a derived label is not in
    /// the framework vocabulary, or the assembled world fails [`World::validate`].
    async fn extract_structure(&self, text: &str) -> Result<World, StructureExtractError>;
}

/// The P-RENDER seam (ADR-0020): turn a [`World`] and its [`Event`]s into
/// medium-specific output — prose, a structured game format, `LitRPG`, detective
/// fiction, or anything else selected by config.
///
/// A `Renderer` implementation is a peer over the same contract, not a special
/// case of another one: `monomyth-text`'s prose renderer and a leaner medium
/// (e.g. `monomyth-render-terse`) both implement this trait directly, and the
/// composition root (`monomyth-cli`, a future launcher) picks one by
/// configuration, never by a hard-coded medium branch anywhere else. Rendering
/// is synchronous and pure — no IO, no network, no async runtime — because it is
/// a deterministic projection of the already-materialized [`World`], unlike
/// [`PassageRetriever`]/[`Classifier`]/[`Extractor`], which cross an IO or model
/// boundary.
///
/// # Genre-free by design
///
/// This trait takes **no** `monomyth-genre`/`monomyth-config` type. Medium
/// selection happens once, at the composition root, by constructing the chosen
/// `Box<dyn Renderer>`; the trait itself only ever sees the pure contract
/// (`World`/`Event`). This keeps `monomyth-contracts` free of an edge onto
/// `monomyth-genre` — mirroring how [`Extractor`] takes only a [`World`] and
/// leaves config/genre wiring to its caller — and it is what makes the "no medium
/// branch outside the composition root" property checkable: a `Renderer` impl
/// cannot special-case a genre from inside the trait method, because the genre
/// never crosses the seam.
pub trait Renderer: Send + Sync {
    /// Render the world's introduction banner (title, seed, or equivalent).
    fn intro(&self, world: &World) -> String;

    /// Render the player's current location.
    fn location(&self, world: &World) -> String;

    /// Render a single [`Event`] as one line of medium-specific output.
    fn event(&self, event: &Event, world: &World) -> String;

    /// Render a slice of [`Event`]s, in order.
    fn events(&self, events: &[Event], world: &World) -> String;

    /// Render the branching narrative structure: the spine, fork points, and
    /// endings.
    fn structure(&self, world: &World) -> String;

    /// Render the choices open at the current narrative cursor.
    fn choices(&self, world: &World) -> String;
}
