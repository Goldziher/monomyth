//! The RAG base layer: a backend-agnostic [`VectorStore`] trait, a neutral
//! type + filter + query IR, and a generic ingest/retrieve pipeline that
//! composes xberg core primitives.
//!
//! Absorbed in-tree from the former standalone `xberg-rag` crate (WS-0): the
//! vector-store contract, filter/query IR, in-memory and sqlite backends, and
//! the ingest/retrieve pipeline are now a submodule of `monomyth-knowledge`
//! rather than a separate published dependency. `monomyth-knowledge` is the
//! only consumer, so the `vector-store` / `in-memory` / `sqlite` / `pipeline`
//! / `pipeline-embeddings` layers that used to be feature-gated are always
//! compiled in. The streaming answer surface (over `liter-llm`) was dropped
//! entirely — monomyth never used it.
//!
//! Optional convenience wiring for reranking / keyword extraction stays
//! behind narrow `monomyth-knowledge` features (`rag-reranker`,
//! `rag-keywords`, `rag-ner-llm`, `rag-ner-onnx`) so a default build incurs no
//! extra pull-in of ONNX Runtime or NER models beyond what embeddings already
//! require.
//!
//! [`InMemoryVectorStore`]: crate::rag::backends::memory::InMemoryVectorStore

pub mod backends;
pub mod capability;
pub mod error;
pub mod filter;
pub mod pipeline;
pub mod query;
pub mod registry;
mod scoring;
pub mod store;
pub mod types;

pub use capability::Capabilities;
pub use error::{ComplexityKind, RagError, RagResult};
pub use filter::{Filter, FilterField, FilterNamespace};
pub use query::{MAX_TOP_K, RetrieveMode, RetrieveOutput, RetrieveQuery};
pub use registry::{
    VectorStoreRegistry, clear_vector_stores, get_vector_store, list_vector_stores,
    register_vector_store, unregister_vector_store, vector_store_registry,
};
pub use store::VectorStore;
pub use types::{
    ChunkId, ChunkRecord, CollectionSpec, CollectionStats, DistanceMetric, DocumentId,
    DocumentRecord, DocumentSummary, IndexMethod, MultiVector, PrimaryScore, RetrievedChunk,
    SparseVector,
};

pub use backends::memory::InMemoryVectorStore;
