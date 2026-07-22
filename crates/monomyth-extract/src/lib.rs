//! Extraction proof-of-concept slices (ADR-0018).
//!
//! This crate implements the `monomyth-contracts` extraction seams
//! ([`Classifier`](monomyth_contracts::Classifier),
//! [`Extractor`](monomyth_contracts::Extractor),
//! [`StructureExtractor`](monomyth_contracts::StructureExtractor)) with two
//! independent slices:
//!
//! - The **classify-only** slice ([`RagSoftmaxClassifier`],
//!   [`StageReclassifyingExtractor`], ADR-0023 Phase B4b) is LLM-free and
//!   corpus-agnostic: it fuses the text under classification with each
//!   candidate label's own descriptor, retrieves scored evidence for the fused
//!   query, and turns the per-label evidence totals into a softmax
//!   distribution. [`RagSoftmaxClassifier`] is generic over any
//!   [`PassageRetriever`](monomyth_contracts::PassageRetriever), so it is
//!   exercised in tests against deterministic fakes and, in production,
//!   against an adapter over `monomyth-knowledge` that lives in the CLI, not
//!   here. Keeping it free of `monomyth-knowledge` and its xberg/ONNX backend
//!   is what makes it hermetic: exercising it never touches a vector store, an
//!   embedder, or the network.
//! - The **minimal structure** slice ([`MinimalStructureExtractor`], Phase 3)
//!   derives structure — not just a label — directly from raw source text via
//!   a single structured LLM call over `monomyth-llm`. Its own tests stay
//!   hermetic by injecting `monomyth-llm`'s cassette `ReplayBackend` rather
//!   than a live provider — no network, no API key.
//!
//! # What lives here
//!
//! - [`RagSoftmaxClassifier`] — a `Classifier<MonomythStage>` over a fused
//!   retrieval query per candidate stage.
//! - [`StageReclassifyingExtractor`] — the structure-preserving `Extractor`
//!   slice: given a skeleton [`World`](monomyth_core::World) whose narrative
//!   structure is already correct, re-derive only the scored `stage` axis per
//!   node.
//! - [`MinimalStructureExtractor`] — the `StructureExtractor` slice: derive a
//!   minimal valid [`World`](monomyth_core::World) — one node, one location —
//!   directly from raw source text via a single structured LLM call, in
//!   contrast to [`StageReclassifyingExtractor`]'s classify-only
//!   re-derivation over an already-structured skeleton.

mod classifier;
mod extractor;
mod structure_extractor;

pub use classifier::RagSoftmaxClassifier;
pub use extractor::StageReclassifyingExtractor;
pub use structure_extractor::MinimalStructureExtractor;
