//! The hermetic, LLM-free RAG-softmax extraction proof-of-concept
//! (ADR-0018/ADR-0023 Phase B4b).
//!
//! This crate implements the `monomyth-contracts` seams
//! ([`Classifier`](monomyth_contracts::Classifier),
//! [`Extractor`](monomyth_contracts::Extractor)) with a concrete, corpus-agnostic
//! strategy: fuse the text under classification with each candidate label's own
//! descriptor, retrieve scored evidence for the fused query, and turn the
//! per-label evidence totals into a softmax distribution. Nothing here touches
//! an LLM, a vector-store backend, or the network — [`RagSoftmaxClassifier`] is
//! generic over any [`PassageRetriever`](monomyth_contracts::PassageRetriever),
//! so it is exercised in tests against deterministic fakes and, in production,
//! against an adapter over `monomyth-knowledge` that lives in the CLI, not
//! here. Keeping this crate free of `monomyth-knowledge` and its xberg/ONNX
//! backend is what makes it hermetic: `cargo test -p monomyth-extract` never
//! touches a vector store, an embedder, or the network.
//!
//! # What lives here
//!
//! - [`RagSoftmaxClassifier`] — a `Classifier<MonomythStage>` over a fused
//!   retrieval query per candidate stage.
//! - [`StageReclassifyingExtractor`] — the structure-preserving `Extractor`
//!   slice: given a skeleton [`World`](monomyth_core::World) whose narrative
//!   structure is already correct, re-derive only the scored `stage` axis per
//!   node.

mod classifier;
mod extractor;

pub use classifier::RagSoftmaxClassifier;
pub use extractor::StageReclassifyingExtractor;
