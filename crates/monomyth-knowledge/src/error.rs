//! Error model for the knowledge layer.

use thiserror::Error;
use xberg_rag::RagError;

use crate::ledger::Namespace;

/// Errors raised by the ship-gated knowledge layer.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum KnowledgeError {
    /// The embedded corpus manifest could not be parsed.
    #[error("failed to parse the embedded corpus manifest")]
    Manifest(#[from] serde_json::Error),

    /// An ingest referenced a source id that is not declared in the ledger.
    #[error("source '{id}' is not declared in the corpus ledger")]
    UndeclaredSource {
        /// The offending source id.
        id: String,
    },

    /// The ship gate refused a non-ship source. Nothing outside the `ship`
    /// namespace may ever enter the shippable store.
    #[error(
        "refused to ingest non-ship source '{id}' (namespace: {namespace:?}) into the shippable store"
    )]
    RefusedNonShip {
        /// The offending source id.
        id: String,
        /// The namespace that triggered the refusal.
        namespace: Namespace,
    },

    /// A vector-store or pipeline operation failed.
    #[error("vector store operation failed ({context})")]
    Store {
        /// What was being attempted.
        context: String,
        /// The underlying RAG error.
        #[source]
        source: RagError,
    },

    /// A retrieved chunk was missing a licensing metadata field the invariant
    /// depends on (source id / namespace / license), or its parent document.
    #[error("retrieved chunk from collection '{collection}' is missing the '{field}' field")]
    MissingMetadata {
        /// The collection the chunk came from.
        collection: String,
        /// Which required field was absent.
        field: &'static str,
    },

    /// A licensing metadata field was present but could not be parsed (e.g. an
    /// unknown namespace token). Distinct from [`KnowledgeError::MissingMetadata`]
    /// so a malformed tag is not misreported as absent.
    #[error("retrieved chunk from collection '{collection}' has a malformed '{field}' field")]
    MalformedMetadata {
        /// The collection the chunk came from.
        collection: String,
        /// Which field failed to parse.
        field: &'static str,
    },

    /// A chunk retrieved from the shippable collection carried a non-`ship`
    /// namespace tag — the fail-closed third enforcement layer. Nothing that is
    /// not provably `ship` may be surfaced, even if it reached the ship
    /// collection out of band.
    #[error(
        "chunk from collection '{collection}' is tagged {found:?}, not Ship; refusing to surface it"
    )]
    NamespaceViolation {
        /// The collection the chunk came from.
        collection: String,
        /// The namespace actually found on the document.
        found: Namespace,
    },

    /// A retrieved chunk requested with content had no content — refuse to
    /// surface a blank passage rather than defaulting to the empty string.
    #[error("retrieved chunk from collection '{collection}' has no content")]
    EmptyContent {
        /// The collection the chunk came from.
        collection: String,
    },
}

impl KnowledgeError {
    /// Wrap a [`RagError`] with the operation context that produced it.
    pub(crate) fn store(context: impl Into<String>, source: RagError) -> Self {
        Self::Store {
            context: context.into(),
            source,
        }
    }
}
