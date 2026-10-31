//! Ship-gated RAG over the in-tree [`rag`] base layer: the license ledger,
//! ship-gated ingestion, and ship-filtered retrieval.
//!
//! This crate owns the vector store, the embedded license ledger
//! ([`Ledger`], from `corpus/manifest.json`), and the two operations that must
//! enforce the hard commercial licensing invariant:
//!
//! - **Ingestion is ship-gated.** [`Knowledge::ingest`] admits only sources whose
//!   ledger [`Namespace`] is [`Namespace::Ship`] into the surfaceable store; any
//!   `reference` source is refused with [`KnowledgeError::RefusedNonShip`], and an
//!   unknown source id with [`KnowledgeError::UndeclaredSource`]. Nothing outside
//!   the `ship` namespace can ever enter the shippable collection. The inverse
//!   path, [`Knowledge::ingest_reference`], admits only `reference` sources into
//!   the reference collection (priors only, never surfaced) — the reference-ingest
//!   half of ADR-0016's build-time synthesis pipeline.
//! - **Surfaceable retrieval is ship-filtered.** [`Knowledge::retrieve`] with a
//!   surfaceable query targets the `ship` collection *and* applies the
//!   `doc.metadata.namespace = "ship"` filter — defense in depth (collection and
//!   filter), so a wrongly tagged document still cannot be surfaced.
//!
//! ```no_run
//! use std::path::Path;
//! use monomyth_knowledge::{IngestInput, Knowledge, KnowledgeQuery};
//!
//! # async fn demo() -> Result<(), monomyth_knowledge::KnowledgeError> {
//! let knowledge = Knowledge::open(Path::new("corpus.db")).await?;
//! knowledge
//!     .ingest("polti", IngestInput::new("The Suppliant implores a Power in authority."))
//!     .await?;
//! let passages = knowledge
//!     .retrieve(KnowledgeQuery::surfaceable("a plea to a powerful protector", 5))
//!     .await?;
//! assert!(passages.iter().all(|p| p.namespace == monomyth_knowledge::Namespace::Ship));
//! # Ok(())
//! # }
//! ```

// `unsafe_code` is `deny`, not `forbid`, only because `rag::backends::sqlite` ~keep
// carries one pre-existing, narrowly-scoped `unsafe` block (registering the ~keep
// sqlite-vec extension via `sqlite3_auto_extension`), inherited unchanged ~keep
// from the former `xberg-rag` crate. Every other module in this crate must ~keep
// stay unsafe-free; a new `unsafe` block anywhere outside that one function ~keep
// is a bug, not a style choice. ~keep
#![deny(unsafe_code)]

#[cfg(feature = "acquire")]
pub mod acquire;
mod audit;
mod enrichment;
mod error;
mod ledger;
#[allow(
    clippy::pedantic,
    reason = "inherited RAG base layer; lint burn-down tracked as follow-up"
)]
pub mod rag;

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde_json::Value;
use xberg::ChunkingConfig;

use crate::rag::backends::sqlite::SqliteVectorStore;
use crate::rag::pipeline::{
    CoreEmbedder, Embedder, IngestRequest, RagPipelineConfig, ingest_document,
    retrieve as pipeline_retrieve,
};
use crate::rag::{
    CollectionSpec, DocumentId, Filter, FilterField, RagError, RetrieveMode, RetrieveQuery,
    RetrievedChunk,
};

#[cfg(feature = "acquire")]
pub use crate::acquire::{
    AcquireMode, BuildOptions, BuildReport, SourceOutcome, SourceReport, build_corpus,
    inspect_corpus,
};
pub use crate::enrichment::{EntityEnrichment, KeywordEnrichment};
pub use crate::error::KnowledgeError;
pub use crate::ledger::{
    Ledger, Namespace, REFERENCE_COLLECTION, SHIP_COLLECTION, SourceEntry, Tier, USER_COLLECTION,
};

/// Embedding dimension of the default (`balanced`) `CoreEmbedder` preset.
///
/// Collections are created at this dimension; a test embedder must emit vectors
/// of this width to match.
pub const EMBEDDING_DIM: u32 = 768;

/// Over-fetch factor for reference retrieval: the reference path requests
/// `top_k * REFERENCE_OVERFETCH` candidates so near-duplicate chunks can be
/// dropped before the final `top_k` is taken. A single long source chunked with
/// overlap otherwise returns several near-identical passages, starving the
/// distillation of *distinct* grounding.
const REFERENCE_OVERFETCH: u32 = 3;

/// Maximum number of query-derived keyword predicates combined into the
/// reference-path keyword filter (WS-C slice 2). Salient query terms beyond this
/// are dropped; the cap bounds the disjunction's per-candidate evaluation and
/// keeps it comfortably within the filter IR's `MAX_FILTER_NODES` complexity cap.
const MAX_QUERY_KEYWORD_PREDICATES: usize = 8;

/// The whitelisted document-level filter field carrying a document's extracted
/// keywords, addressed for the reference-path [`Filter::ArrayContains`] narrowing.
const DOC_KEYWORDS_FIELD: &str = "doc.keywords";

/// The store name registered for the knowledge vector store.
const STORE_NAME: &str = "monomyth";

/// Metadata key carrying the declaring source id on each stored document.
const META_SOURCE_ID: &str = "source_id";
/// Metadata key carrying the trust-domain namespace on each stored document.
const META_NAMESPACE: &str = "namespace";
/// Metadata key carrying the license string on each stored document.
const META_LICENSE: &str = "license";
/// Metadata key carrying the license tier on each stored document.
const META_TIER: &str = "tier";
/// Metadata key carrying the content domain on each stored document.
const META_DOMAIN: &str = "domain";
/// Metadata key carrying the source URL on each stored document, when known.
const META_URL: &str = "url";
/// Metadata key carrying the content checksum on each stored document, when known.
const META_CHECKSUM: &str = "checksum";
/// Metadata key carrying the retrieval date on each stored document, when known.
const META_RETRIEVED: &str = "retrieved";

/// Our input type for a single ingest, deliberately narrower than the pipeline's
/// [`IngestRequest`]: licensing metadata is supplied by the ledger, not the
/// caller, so it cannot be spoofed at the call site.
#[derive(Debug, Clone, Default)]
pub struct IngestInput {
    /// The full text to chunk, embed, and store.
    pub full_text: String,
    /// Optional human-readable title.
    pub title: Option<String>,
    /// Optional source URI (path, URL, object key).
    pub source_uri: Option<String>,
    /// Optional content checksum, carried as provenance (ADR-0005).
    pub checksum: Option<String>,
    /// Optional retrieval date, carried as provenance (ADR-0005).
    pub retrieved: Option<String>,
}

impl IngestInput {
    /// Construct an input from full text, with no title, URI, or provenance.
    #[must_use]
    pub fn new(full_text: impl Into<String>) -> Self {
        Self {
            full_text: full_text.into(),
            title: None,
            source_uri: None,
            checksum: None,
            retrieved: None,
        }
    }
}

/// A retrieval request against the knowledge layer.
#[derive(Debug, Clone)]
pub struct KnowledgeQuery {
    /// The query text.
    pub text: String,
    /// Maximum number of passages to return.
    pub top_k: u32,
    /// Whether the results may be surfaced verbatim. When `true`, retrieval is
    /// ship-gated (ship collection + ship filter). When `false`, retrieval draws
    /// on the reference collection for priors only.
    pub surfaceable: bool,
}

impl KnowledgeQuery {
    /// A ship-gated query whose results may be shown verbatim.
    #[must_use]
    pub fn surfaceable(text: impl Into<String>, top_k: u32) -> Self {
        Self {
            text: text.into(),
            top_k,
            surfaceable: true,
        }
    }

    /// A reference-only query whose results inform generation but are never
    /// shown verbatim.
    ///
    /// The reference collection is populated by [`Knowledge::ingest_reference`]
    /// (ADR-0016); passages it returns are licensed for priors only, so callers
    /// must gate any rendering on [`Passage::is_surfaceable`] (always `false`
    /// for a reference passage).
    #[must_use]
    pub fn reference(text: impl Into<String>, top_k: u32) -> Self {
        Self {
            text: text.into(),
            top_k,
            surfaceable: false,
        }
    }
}

/// A retrieved passage, carrying the licensing provenance needed to decide
/// whether it may be surfaced.
///
/// A passage from a reference query is licensed for priors only and must never
/// be shown verbatim; check [`Passage::is_surfaceable`] before rendering.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Passage {
    /// The chunk text.
    pub text: String,
    /// The declaring source id.
    pub source_id: String,
    /// Retrieval relevance score.
    pub score: f32,
    /// The source's trust domain.
    pub namespace: Namespace,
    /// The source's license string.
    pub license: String,
    /// The source URL, when known (ADR-0005 provenance).
    pub url: Option<String>,
    /// The content checksum, when known (ADR-0005 provenance).
    pub checksum: Option<String>,
    /// The retrieval date, when known (ADR-0005 provenance).
    pub retrieved: Option<String>,
}

impl Passage {
    /// Whether this passage may be shown to a user verbatim.
    ///
    /// Only `ship`-namespace passages are surfaceable; a reference passage
    /// informs generation as a prior but must never be redistributed. Callers
    /// that render passage text should gate on this.
    #[must_use]
    pub fn is_surfaceable(&self) -> bool {
        self.namespace == Namespace::Ship
    }
}

/// The ship-gated knowledge layer: vector store + embedder + license ledger.
pub struct Knowledge {
    store: Arc<dyn crate::rag::VectorStore>,
    embedder: Arc<dyn Embedder>,
    ledger: Ledger,
    chunking: ChunkingConfig,
    keyword_enrichment: KeywordEnrichment,
    entity_enrichment: EntityEnrichment,
}

impl fmt::Debug for Knowledge {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Knowledge")
            .field("store", &self.store.name())
            .field("sources", &self.ledger.len())
            .finish_non_exhaustive()
    }
}

impl Knowledge {
    /// Open a file-backed knowledge layer at `db_path`, using the embedded ledger,
    /// a local ONNX [`CoreEmbedder`], and semantic chunking.
    ///
    /// # Errors
    ///
    /// Returns [`KnowledgeError::Manifest`] if the embedded ledger is malformed,
    /// or [`KnowledgeError::Store`] if the store cannot be opened or its
    /// collections created.
    pub async fn open(db_path: &Path) -> Result<Self, KnowledgeError> {
        let store = SqliteVectorStore::open(STORE_NAME, db_path.to_string_lossy().into_owned())
            .await
            .map_err(|error| KnowledgeError::store("opening sqlite store", error))?;
        let embedder = CoreEmbedder {
            config: xberg::EmbeddingConfig::default(),
        };
        let knowledge = Self {
            store: Arc::new(store),
            embedder: Arc::new(embedder),
            ledger: Ledger::load_embedded()?,
            chunking: semantic_chunking(),
            keyword_enrichment: KeywordEnrichment::default(),
            entity_enrichment: EntityEnrichment::default(),
        };
        knowledge.ensure_collection(SHIP_COLLECTION).await?;
        knowledge.ensure_collection(REFERENCE_COLLECTION).await?;
        Ok(knowledge)
    }

    /// Construct a knowledge layer from injected parts — the test seam.
    ///
    /// Uses a plain text [`ChunkingConfig`] so no embedding model is needed for
    /// chunking; pair it with an in-memory store and a fake embedder in tests.
    #[must_use]
    pub fn with(
        store: Arc<dyn crate::rag::VectorStore>,
        embedder: Arc<dyn Embedder>,
        ledger: Ledger,
    ) -> Self {
        Self {
            store,
            embedder,
            ledger,
            chunking: ChunkingConfig::default(),
            keyword_enrichment: KeywordEnrichment::default(),
            entity_enrichment: EntityEnrichment::default(),
        }
    }

    /// Return `self` with keyword enrichment reconfigured.
    ///
    /// Defaults to disabled (see [`KeywordEnrichment`]); a caller opts in with
    /// [`KeywordEnrichment::enabled`] or a custom [`xberg::KeywordConfig`].
    /// Extraction only runs when the `rag-keywords` feature is compiled in —
    /// enabling this knob without the feature has no effect.
    #[must_use]
    pub fn with_keyword_enrichment(mut self, keyword_enrichment: KeywordEnrichment) -> Self {
        self.keyword_enrichment = keyword_enrichment;
        self
    }

    /// Return `self` with named-entity enrichment reconfigured.
    ///
    /// Defaults to disabled (see [`EntityEnrichment`]); a caller opts in with
    /// [`EntityEnrichment::with_backend`] or [`EntityEnrichment::llm`]. Extraction
    /// only runs when the `rag-ner-llm` feature is compiled in — enabling this
    /// knob without the feature has no effect.
    #[must_use]
    pub fn with_entity_enrichment(mut self, entity_enrichment: EntityEnrichment) -> Self {
        self.entity_enrichment = entity_enrichment;
        self
    }

    /// Ingest `input` under the ledger-declared source `source_id` into the
    /// surfaceable ship collection.
    ///
    /// The ship gate rejects any source not declared in the ledger
    /// ([`KnowledgeError::UndeclaredSource`]) and any source outside the `ship`
    /// namespace ([`KnowledgeError::RefusedNonShip`]) before any text is stored.
    ///
    /// # Errors
    ///
    /// Ship-gate errors as above, or [`KnowledgeError::Store`] on a store failure.
    pub async fn ingest(
        &self,
        source_id: &str,
        input: IngestInput,
    ) -> Result<DocumentId, KnowledgeError> {
        let entry = self.require_source(source_id)?;
        if entry.namespace != Namespace::Ship {
            return Err(KnowledgeError::RefusedNonShip {
                id: source_id.to_owned(),
                namespace: entry.namespace,
            });
        }
        self.ingest_into(SHIP_COLLECTION, source_id, entry, input)
            .await
    }

    /// Ingest `input` under the ledger-declared source `source_id` into the
    /// reference collection, for generation priors only (ADR-0016).
    ///
    /// This is the inverse gate of [`Self::ingest`]: it admits **only** sources
    /// whose ledger [`Namespace`] is [`Namespace::Reference`], rejecting an
    /// undeclared source ([`KnowledgeError::UndeclaredSource`]) and a `ship`
    /// source ([`KnowledgeError::RefusedNonReference`]) before any text is
    /// stored. Reference material informs generation but is never surfaceable:
    /// it lands in the reference collection, which [`Self::retrieve`]'s
    /// surfaceable path never targets and whose namespace tag
    /// [`Passage::is_surfaceable`] reports as unshowable. The commercial
    /// licensing invariant (ADR-0005) therefore holds by construction — a
    /// reference source cannot reach the ship collection through this path.
    ///
    /// # Errors
    ///
    /// Reference-gate errors as above, or [`KnowledgeError::Store`] on a store
    /// failure.
    pub async fn ingest_reference(
        &self,
        source_id: &str,
        input: IngestInput,
    ) -> Result<DocumentId, KnowledgeError> {
        let entry = self.require_source(source_id)?;
        if entry.namespace != Namespace::Reference {
            return Err(KnowledgeError::RefusedNonReference {
                id: source_id.to_owned(),
                namespace: entry.namespace,
            });
        }
        self.ingest_into(REFERENCE_COLLECTION, source_id, entry, input)
            .await
    }

    /// Best-effort keyword extraction for `text`, run only when enrichment is
    /// enabled and the `rag-keywords` feature is compiled in. `context` is a
    /// short label (the ingest `source_id` or `"reference query"`) attached to
    /// the failure log. Extraction is never fatal: a failure is logged and
    /// treated as "no keywords" rather than propagated, and the feature-off /
    /// disabled case returns empty without attempting extraction at all — so
    /// default behavior (and every existing cassette/golden) is byte-for-byte
    /// unchanged.
    #[cfg_attr(not(feature = "rag-keywords"), allow(unused_variables))]
    fn extract_keywords_best_effort(&self, text: &str, context: &str) -> Vec<String> {
        if !self.keyword_enrichment.enabled {
            return Vec::new();
        }
        #[cfg(feature = "rag-keywords")]
        {
            match crate::rag::pipeline::extract_keywords(text, &self.keyword_enrichment.config) {
                Ok(keywords) => keywords,
                Err(error) => {
                    tracing::warn!(
                        context,
                        error = %error,
                        "best-effort keyword extraction failed; proceeding with no keywords"
                    );
                    Vec::new()
                }
            }
        }
        #[cfg(not(feature = "rag-keywords"))]
        {
            Vec::new()
        }
    }

    /// Best-effort NER over `text` → deduped, sorted entity surface strings, run
    /// only when enabled and `rag-ner-llm` is compiled. Never fatal to an ingest:
    /// a backend error is logged and treated as "no entities". Returns `Vec::new()`
    /// when disabled / feature-off, so default behavior is byte-for-byte unchanged.
    #[cfg_attr(not(feature = "rag-ner-llm"), allow(unused_variables))]
    // `async` is genuinely needed: `xberg::detect_entities` is async and is ~keep
    // `.await`ed in the `rag-ner-llm` body below. With the feature off there is ~keep
    // no await in this stub body, so `clippy::unused_async` (and, in a trait impl, ~keep
    // `clippy::unused_async_trait_impl`) fires spuriously — allow them only in that ~keep
    // configuration rather than dropping `async` and forcing every call site to ~keep
    // branch on the feature. ~keep
    #[cfg_attr(
        not(feature = "rag-ner-llm"),
        allow(clippy::unused_async, clippy::unused_async_trait_impl)
    )]
    async fn extract_entities_best_effort(&self, text: &str, context: &str) -> Vec<String> {
        if !self.entity_enrichment.enabled {
            return Vec::new();
        }
        #[cfg(feature = "rag-ner-llm")]
        {
            let Some(backend) = &self.entity_enrichment.backend else {
                return Vec::new();
            };
            match xberg::detect_entities(text, backend.as_ref(), &self.entity_enrichment.categories)
                .await
            {
                Ok(entities) => entities
                    .into_iter()
                    .map(|entity| entity.text)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                Err(error) => {
                    tracing::warn!(
                        context,
                        error = %error,
                        "best-effort NER failed; proceeding with no entities"
                    );
                    Vec::new()
                }
            }
        }
        #[cfg(not(feature = "rag-ner-llm"))]
        {
            Vec::new()
        }
    }

    /// Build the reference-path keyword filter from the salient terms of `text`,
    /// or `None` when enrichment is disabled, the `rag-keywords` feature is
    /// compiled out, or the query yields no keywords.
    ///
    /// The filter is a disjunction of [`Filter::ArrayContains`] over
    /// `doc.keywords` (capped at [`MAX_QUERY_KEYWORD_PREDICATES`]): a reference
    /// document is a candidate when it shares *any* salient term with the query,
    /// biasing the priors toward lexically-aligned grounding. Used on the
    /// reference path only ([`retrieve_reference`](Self::retrieve_reference)) and
    /// always degradable — the ship path is never keyword-filtered, so this can
    /// never perturb surfaceable retrieval.
    fn reference_keyword_filter(&self, text: &str) -> Option<Filter> {
        let mut predicates: Vec<Filter> = self
            .extract_keywords_best_effort(text, "reference query")
            .into_iter()
            .take(MAX_QUERY_KEYWORD_PREDICATES)
            .map(|keyword| Filter::ArrayContains {
                field: FilterField(DOC_KEYWORDS_FIELD.to_owned()),
                value: Value::String(keyword),
            })
            .collect();
        match predicates.len() {
            0 => None,
            1 => predicates.pop(),
            _ => Some(Filter::Or {
                filters: predicates,
            }),
        }
    }

    /// Look up `source_id` in the ledger or fail with
    /// [`KnowledgeError::UndeclaredSource`]. Shared by both ingest gates so an
    /// undeclared source is refused identically before the namespace check.
    fn require_source(&self, source_id: &str) -> Result<&SourceEntry, KnowledgeError> {
        self.ledger
            .get(source_id)
            .ok_or_else(|| KnowledgeError::UndeclaredSource {
                id: source_id.to_owned(),
            })
    }

    /// Chunk, embed, and store `input` into `collection` with the ledger-derived
    /// licensing metadata for `entry`. The namespace gate is the caller's
    /// responsibility ([`Self::ingest`] / [`Self::ingest_reference`]); this
    /// helper only runs once a source has been admitted to `collection`.
    async fn ingest_into(
        &self,
        collection: &str,
        source_id: &str,
        entry: &SourceEntry,
        input: IngestInput,
    ) -> Result<DocumentId, KnowledgeError> {
        let metadata = ingest_metadata(source_id, entry, &input);
        let keywords = self.extract_keywords_best_effort(&input.full_text, source_id);
        let entities = self
            .extract_entities_best_effort(&input.full_text, source_id)
            .await;

        let mut request = IngestRequest {
            full_text: input.full_text,
            title: input.title,
            source_uri: input.source_uri,
            metadata,
            keywords,
            ..IngestRequest::default()
        };
        if !entities.is_empty() {
            request.entities = Value::Array(entities.into_iter().map(Value::String).collect());
        }

        self.ensure_collection(collection).await?;
        let config = RagPipelineConfig {
            chunking: &self.chunking,
        };
        ingest_document(
            Arc::clone(&self.store),
            collection,
            request,
            &config,
            self.embedder.as_ref(),
        )
        .await
        .map_err(|error| KnowledgeError::store("ingesting document", error))
    }

    /// Retrieve passages for `query`.
    ///
    /// A surfaceable query is ship-gated: it targets the `ship` collection and
    /// applies the `doc.metadata.namespace = "ship"` filter (collection *and*
    /// filter, defense in depth). A reference query draws on the reference
    /// collection for priors only — those passages are **not** licensed for
    /// verbatim display; callers must gate rendering on
    /// [`Passage::is_surfaceable`].
    ///
    /// # Errors
    ///
    /// [`KnowledgeError::Store`] on a store failure, or
    /// [`KnowledgeError::MissingMetadata`] if a stored chunk lacks the licensing
    /// metadata every ingest writes.
    pub async fn retrieve(&self, query: KnowledgeQuery) -> Result<Vec<Passage>, KnowledgeError> {
        if query.surfaceable {
            self.retrieve_surfaceable(&query.text, query.top_k).await
        } else {
            self.retrieve_reference(&query.text, query.top_k).await
        }
    }

    /// Embed each string in `texts` into a dense vector, one per input, in order.
    ///
    /// Exposes the layer's own embedder for callers that need to score text
    /// similarity *outside* retrieval — notably the semantic evaluation axis,
    /// which compares candidate text against grounding by embedding cosine. It
    /// uses the query-side embedding path (the same prefix
    /// [`retrieve`](Self::retrieve) applies to a query), so a returned vector is
    /// directly comparable to the vectors retrieval scores against.
    ///
    /// # Errors
    ///
    /// Returns [`KnowledgeError::Store`] if the underlying embedder fails.
    pub async fn embed_texts(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>, KnowledgeError> {
        self.embedder
            .embed_query(texts)
            .await
            .map_err(|error| KnowledgeError::store("embedding texts", error))
    }

    /// The surfaceable (ship) retrieval path: ship collection + ship filter, plain
    /// vector search. Deliberately left byte-for-byte unchanged so recorded
    /// content-fill fixtures (which key on the exact grounding a query returns)
    /// stay valid.
    async fn retrieve_surfaceable(
        &self,
        text: &str,
        top_k: u32,
    ) -> Result<Vec<Passage>, KnowledgeError> {
        self.ensure_collection(SHIP_COLLECTION).await?;
        let query = RetrieveQuery {
            query_text: Some(text.to_owned()),
            filter: Some(ship_filter()),
            include_content: true,
            include_document: true,
            ..RetrieveQuery::vector(top_k)
        };
        let chunks = self
            .run_retrieve(SHIP_COLLECTION, query)
            .await
            .map_err(|error| KnowledgeError::store("retrieving chunks", error))?;
        chunks
            .into_iter()
            .map(|chunk| passage_from_chunk(chunk, SHIP_COLLECTION))
            .collect()
    }

    /// The reference (priors-only) retrieval path: hybrid (dense + lexical)
    /// search with over-fetch + near-duplicate dedup for *distinct* coverage. A
    /// single long source chunked with overlap otherwise returns several
    /// near-identical passages, starving the distillation of distinct grounding.
    ///
    /// Robust hybrid (ADR-0025): xberg's hybrid mode otherwise passes `query_text`
    /// straight to FTS5's `MATCH` parser, which errors on ordinary punctuation in
    /// a natural-language query (an apostrophe, colon, or comma). We sidestep that
    /// by pre-embedding the **raw** query ourselves for the dense arm and passing a
    /// **separately FTS5-escaped** term query ([`fts5_match_query`]) for the
    /// lexical arm — because `query_vector` is then set, xberg does not re-embed
    /// and uses `query_text` only for FTS. A backend that does not support hybrid
    /// (e.g. the in-memory store) reports [`RagError::UnsupportedMode`], on which
    /// we fall back to plain vector search with the same pre-computed vector.
    ///
    /// The surfaceable (ship) path is deliberately **not** changed — it stays on
    /// plain vector so recorded content-fill fixtures keep matching.
    async fn retrieve_reference(
        &self,
        text: &str,
        top_k: u32,
    ) -> Result<Vec<Passage>, KnowledgeError> {
        self.ensure_collection(REFERENCE_COLLECTION).await?;
        let fetch_k = top_k.saturating_mul(REFERENCE_OVERFETCH).max(top_k);

        // Pre-embed the raw query for the dense arm (query-side prefix preserved ~keep
        // by `embed_query`); an FTS5-escaped term string drives the lexical arm. ~keep
        // An all-punctuation query escapes to no terms, degrading to vector-only. ~keep
        let query_vector = self
            .embedder
            .embed_query(vec![text.to_owned()])
            .await
            .map_err(|error| KnowledgeError::store("embedding reference query", error))?
            .pop();
        let escaped = fts5_match_query(text);

        // Reference-path keyword narrowing (WS-C slice 2): when enrichment is on, ~keep
        // bias candidates toward reference documents that share a salient query ~keep
        // term. A filter that starves retrieval — no document shares a keyword, ~keep
        // so the filtered fetch comes back empty — degrades to the unfiltered ~keep
        // path, guaranteeing the distiller is never left without grounding it ~keep
        // would otherwise have had. When enrichment is off (the default), the ~keep
        // filter is `None` and this is byte-for-byte the prior unfiltered path. ~keep
        if let Some(filter) = self.reference_keyword_filter(text) {
            let filtered = self
                .run_reference_retrieve(
                    fetch_k,
                    query_vector.clone(),
                    escaped.clone(),
                    Some(filter),
                )
                .await?;
            if !filtered.is_empty() {
                return reference_passages(filtered, top_k);
            }
        }

        let chunks = self
            .run_reference_retrieve(fetch_k, query_vector, escaped, None)
            .await?;

        reference_passages(chunks, top_k)
    }

    /// Run the reference retrieval as hybrid when a lexical query and a store
    /// that supports it are available, falling back to plain vector search on
    /// [`RagError::UnsupportedMode`]. The pre-computed `query_vector` is reused
    /// across both, so the dense arm is identical in either branch.
    async fn run_reference_retrieve(
        &self,
        fetch_k: u32,
        query_vector: Option<Vec<f32>>,
        escaped: Option<String>,
        keyword_filter: Option<Filter>,
    ) -> Result<Vec<RetrievedChunk>, KnowledgeError> {
        let vector_query = |query_vector: Option<Vec<f32>>, filter: Option<Filter>| RetrieveQuery {
            query_vector,
            filter,
            include_content: true,
            include_document: true,
            ..RetrieveQuery::vector(fetch_k)
        };

        let Some(match_query) = escaped else {
            return self
                .run_retrieve(
                    REFERENCE_COLLECTION,
                    vector_query(query_vector, keyword_filter),
                )
                .await
                .map_err(|error| KnowledgeError::store("retrieving chunks", error));
        };

        let hybrid_query = RetrieveQuery {
            mode: RetrieveMode::Hybrid,
            query_text: Some(match_query),
            query_vector: query_vector.clone(),
            filter: keyword_filter.clone(),
            include_content: true,
            include_document: true,
            ..RetrieveQuery::vector(fetch_k)
        };
        match self.run_retrieve(REFERENCE_COLLECTION, hybrid_query).await {
            Ok(chunks) => Ok(chunks),
            Err(RagError::UnsupportedMode { .. }) => self
                .run_retrieve(
                    REFERENCE_COLLECTION,
                    vector_query(query_vector, keyword_filter),
                )
                .await
                .map_err(|error| KnowledgeError::store("retrieving chunks", error)),
            Err(error) => Err(KnowledgeError::store("retrieving chunks", error)),
        }
    }

    /// Run a prepared retrieval against `collection`, returning the raw chunks.
    /// Error mapping is left to the caller so the reference path can inspect a
    /// [`RagError::UnsupportedMode`] and fall back to vector search.
    async fn run_retrieve(
        &self,
        collection: &str,
        query: RetrieveQuery,
    ) -> Result<Vec<RetrievedChunk>, RagError> {
        pipeline_retrieve(
            Arc::clone(&self.store),
            collection,
            query,
            Some(self.embedder.as_ref()),
        )
        .await
    }

    /// The embedded license ledger, for callers (e.g. the acquisition pipeline) that need to
    /// enumerate declared sources.
    #[must_use]
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }

    /// Ensure `collection` exists at the embedding dimension. Idempotent.
    async fn ensure_collection(&self, collection: &str) -> Result<(), KnowledgeError> {
        let spec = CollectionSpec::new(collection, EMBEDDING_DIM);
        self.store
            .ensure_collection(&spec)
            .await
            .map_err(|error| KnowledgeError::store("ensuring collection", error))
    }
}

/// The ship filter: `doc.metadata.namespace = "ship"`.
///
/// The filter whitelist admits only `doc.metadata.*` for free-form tags, so the
/// namespace tag is addressed as `doc.metadata.namespace`. The value is taken
/// from [`Namespace::as_wire`] — the same source of truth ingest writes — so the
/// enforcement filter cannot drift from the stored tag.
fn ship_filter() -> Filter {
    Filter::Eq {
        field: FilterField(format!("doc.metadata.{META_NAMESPACE}")),
        value: Value::String(Namespace::Ship.as_wire().to_owned()),
    }
}

/// Build the stored document metadata for an ingest: the ledger-declared
/// licensing tags (always present) plus the caller-supplied provenance fields
/// (`url`, `checksum`, `retrieved`) — written only when present, so the stored
/// JSON never carries null provenance keys (ADR-0005).
fn ingest_metadata(source_id: &str, entry: &SourceEntry, input: &IngestInput) -> Value {
    let mut metadata = serde_json::json!({
        META_SOURCE_ID: source_id,
        META_NAMESPACE: entry.namespace,
        META_LICENSE: entry.license,
        META_TIER: entry.tier,
        META_DOMAIN: entry.domain,
    });

    let Value::Object(object) = &mut metadata else {
        unreachable!("json!({{...}}) with braces always builds an object");
    };
    if let Some(url) = &input.source_uri {
        object.insert(META_URL.to_owned(), Value::String(url.clone()));
    }
    if let Some(checksum) = &input.checksum {
        object.insert(META_CHECKSUM.to_owned(), Value::String(checksum.clone()));
    }
    if let Some(retrieved) = &input.retrieved {
        object.insert(META_RETRIEVED.to_owned(), Value::String(retrieved.clone()));
    }

    metadata
}

/// Semantic chunking paired with the default embedding model, per the corpus
/// guidance to chunk at topic boundaries.
fn semantic_chunking() -> ChunkingConfig {
    ChunkingConfig {
        chunker_type: xberg::ChunkerType::Semantic,
        embedding: Some(xberg::EmbeddingConfig::default()),
        ..ChunkingConfig::default()
    }
}

/// Finalize reference-collection chunks into passages: near-duplicate dedup down
/// to `top_k`, then convert each surviving chunk. Shared by the keyword-filtered
/// and unfiltered reference paths so both apply identical dedup and conversion.
fn reference_passages(
    chunks: Vec<RetrievedChunk>,
    top_k: u32,
) -> Result<Vec<Passage>, KnowledgeError> {
    dedup_chunks(chunks, top_k as usize)
        .into_iter()
        .map(|chunk| passage_from_chunk(chunk, REFERENCE_COLLECTION))
        .collect()
}

/// Build a [`Passage`] from a retrieved chunk, reading licensing provenance from
/// the parent document's metadata.
/// Drop chunks whose normalized text repeats an earlier (higher-ranked) chunk,
/// then keep at most `limit`. Chunks arrive in descending relevance order, so the
/// first occurrence — the most relevant — is the copy retained.
fn dedup_chunks(chunks: Vec<RetrievedChunk>, limit: usize) -> Vec<RetrievedChunk> {
    let mut seen = std::collections::BTreeSet::new();
    let mut kept = Vec::with_capacity(limit.min(chunks.len()));
    for chunk in chunks {
        let key = normalize_for_dedup(chunk.content.as_deref().unwrap_or_default());
        if seen.insert(key) {
            kept.push(chunk);
            if kept.len() >= limit {
                break;
            }
        }
    }
    kept
}

/// Normalize chunk text for duplicate detection: collapse every whitespace run to
/// a single space, trim, and lowercase. Catches the exact / whitespace-only-different
/// chunks a single overlapping source otherwise yields.
fn normalize_for_dedup(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Turn a natural-language query into a valid FTS5 `MATCH` string for the hybrid
/// lexical arm (ADR-0025), or `None` when there are no usable terms.
///
/// FTS5's `MATCH` parser treats `:`, `,`, `-`, `*`, `(`, `)`, `^`, `"` and a bare
/// apostrophe as syntax, so a raw natural-language query (`"the hero's descent:
/// trial, and return"`) is a syntax error. Wrapping **each** whitespace-split
/// token in double quotes turns it into a literal FTS5 string token where those
/// characters are inert; embedded quotes are escaped by doubling (`"` -> `""`).
/// Tokens are joined with a space (FTS5 implicit AND) — the lexical arm is the
/// *precise* signal, since RRF already unions it with the dense arm. An empty or
/// all-whitespace query yields no tokens and returns `None`, so the caller
/// degrades to plain vector search rather than sending an (also-invalid) empty
/// `MATCH`.
fn fts5_match_query(text: &str) -> Option<String> {
    let quoted: Vec<String> = text
        .split_whitespace()
        .map(|token| format!("\"{}\"", token.replace('"', "\"\"")))
        .collect();
    if quoted.is_empty() {
        None
    } else {
        Some(quoted.join(" "))
    }
}

fn passage_from_chunk(chunk: RetrievedChunk, collection: &str) -> Result<Passage, KnowledgeError> {
    let metadata = chunk
        .document
        .as_ref()
        .map(|doc| doc.metadata.clone())
        .ok_or_else(|| KnowledgeError::MissingMetadata {
            collection: collection.to_owned(),
            field: "document",
        })?;
    build_passage(&metadata, chunk.content, chunk.score, collection)
}

/// The pure core of passage construction: extract licensing provenance from
/// document `metadata`, enforce the fail-closed invariants, and pair it with the
/// chunk text. Split out from [`passage_from_chunk`] so the enforcement can be
/// unit-tested without constructing xberg store types.
///
/// This is the **third** licensing enforcement layer (after the ingest gate and
/// the retrieval filter): a chunk drawn from the ship collection whose namespace
/// tag is anything but `ship`, or that carries no content, is refused rather than
/// surfaced.
fn build_passage(
    metadata: &Value,
    content: Option<String>,
    score: f32,
    collection: &str,
) -> Result<Passage, KnowledgeError> {
    let missing = |field| KnowledgeError::MissingMetadata {
        collection: collection.to_owned(),
        field,
    };

    let source_id = metadata
        .get(META_SOURCE_ID)
        .and_then(Value::as_str)
        .ok_or_else(|| missing(META_SOURCE_ID))?
        .to_owned();
    let namespace_value = metadata
        .get(META_NAMESPACE)
        .ok_or_else(|| missing(META_NAMESPACE))?;
    let namespace: Namespace = serde_json::from_value(namespace_value.clone()).map_err(|_| {
        KnowledgeError::MalformedMetadata {
            collection: collection.to_owned(),
            field: META_NAMESPACE,
        }
    })?;

    let expected = match collection {
        SHIP_COLLECTION => Some(Namespace::Ship),
        REFERENCE_COLLECTION => Some(Namespace::Reference),
        USER_COLLECTION => Some(Namespace::User),
        _ => None,
    };
    if let Some(expected) = expected
        && namespace != expected
    {
        return Err(KnowledgeError::NamespaceViolation {
            collection: collection.to_owned(),
            found: namespace,
        });
    }

    let license = metadata
        .get(META_LICENSE)
        .and_then(Value::as_str)
        .ok_or_else(|| missing(META_LICENSE))?
        .to_owned();
    let text = content.ok_or_else(|| KnowledgeError::EmptyContent {
        collection: collection.to_owned(),
    })?;

    let url = metadata
        .get(META_URL)
        .and_then(Value::as_str)
        .map(str::to_owned);
    let checksum = metadata
        .get(META_CHECKSUM)
        .and_then(Value::as_str)
        .map(str::to_owned);
    let retrieved = metadata
        .get(META_RETRIEVED)
        .and_then(Value::as_str)
        .map(str::to_owned);

    Ok(Passage {
        text,
        source_id,
        score,
        namespace,
        license,
        url,
        checksum,
        retrieved,
    })
}

#[cfg(test)]
mod tests;
