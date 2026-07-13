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

// `unsafe_code` is `deny`, not `forbid`, only because `rag::backends::sqlite`
// carries one pre-existing, narrowly-scoped `unsafe` block (registering the
// sqlite-vec extension via `sqlite3_auto_extension`), inherited unchanged
// from the former `xberg-rag` crate. Every other module in this crate must
// stay unsafe-free; a new `unsafe` block anywhere outside that one function
// is a bug, not a style choice.
#![deny(unsafe_code)]

#[cfg(feature = "acquire")]
pub mod acquire;
mod audit;
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
pub use crate::error::KnowledgeError;
pub use crate::ledger::{
    Ledger, Namespace, REFERENCE_COLLECTION, SHIP_COLLECTION, SourceEntry, Tier,
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

/// Entity category name for a mythological or divine figure, used to seed
/// [`default_mythic_categories`].
#[cfg(feature = "rag-ner-llm")]
const MYTHIC_CATEGORY_DEITY: &str = "Deity";
/// Entity category name for a story's protagonist figure.
#[cfg(feature = "rag-ner-llm")]
const MYTHIC_CATEGORY_HERO: &str = "Hero";
/// Entity category name for a named magical or narratively significant object.
#[cfg(feature = "rag-ner-llm")]
const MYTHIC_CATEGORY_ARTIFACT: &str = "Artifact";
/// Entity category name for a named place of mythic or narrative significance.
#[cfg(feature = "rag-ner-llm")]
const MYTHIC_CATEGORY_REALM: &str = "Realm";

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

/// Runtime knob for best-effort keyword extraction at ingest time.
///
/// Interim home: this is a small runtime setter on [`Knowledge`] rather than a
/// `monomyth.toml` section, pending config wiring in a later slice. Defaults to
/// **disabled**, so default behavior — and every existing cassette/golden — is
/// byte-for-byte unchanged; a caller opts in via
/// [`Knowledge::with_keyword_enrichment`].
///
/// The `config` field (and hence `xberg::KeywordConfig`, which only exists
/// when `xberg/keywords` is compiled) is only present when the `rag-keywords`
/// feature is enabled. With the feature off, `enabled` can still be set but
/// extraction is compiled out entirely, so ingest always proceeds with empty
/// keywords.
#[derive(Debug, Clone, Default)]
pub struct KeywordEnrichment {
    /// Whether ingest should attempt keyword extraction.
    pub enabled: bool,
    /// Extraction configuration, forwarded to `xberg::keywords::extract_keywords`.
    #[cfg(feature = "rag-keywords")]
    pub config: xberg::KeywordConfig,
}

impl KeywordEnrichment {
    /// Enrichment enabled with the default [`xberg::KeywordConfig`].
    #[must_use]
    #[cfg(feature = "rag-keywords")]
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            config: xberg::KeywordConfig::default(),
        }
    }
}

/// Runtime knob for best-effort named-entity extraction at ingest time (WS-C
/// slice 3), mirroring [`KeywordEnrichment`]. Defaults to **disabled**, so
/// default behavior — and every existing cassette/golden — is byte-for-byte
/// unchanged; a caller opts in via [`Knowledge::with_entity_enrichment`].
///
/// The backend and categories (and hence `xberg::NerBackend` /
/// `xberg::EntityCategory`, which only exist when `xberg/ner-llm` is compiled)
/// are only present when the `rag-ner-llm` feature is enabled. With the
/// feature off, `enabled` can still be set but extraction is compiled out
/// entirely, so ingest always proceeds with no entities.
#[derive(Clone, Default)]
pub struct EntityEnrichment {
    /// Whether ingest should attempt named-entity extraction.
    pub enabled: bool,
    /// The NER backend to call, forwarded to `xberg::detect_entities`.
    #[cfg(feature = "rag-ner-llm")]
    backend: Option<Arc<dyn xberg::NerBackend>>,
    /// The entity categories to detect.
    #[cfg(feature = "rag-ner-llm")]
    categories: Vec<xberg::EntityCategory>,
}

impl fmt::Debug for EntityEnrichment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug_struct = formatter.debug_struct("EntityEnrichment");
        debug_struct.field("enabled", &self.enabled);
        #[cfg(feature = "rag-ner-llm")]
        debug_struct.field("has_backend", &self.backend.is_some());
        // `categories` is intentionally omitted (not useful in a log line);
        // `finish_non_exhaustive` documents that this Debug impl is a summary,
        // not a full field dump, satisfying `clippy::missing_fields_in_debug`.
        debug_struct.finish_non_exhaustive()
    }
}

impl EntityEnrichment {
    /// Enrichment enabled with an explicit backend and category set.
    ///
    /// The narrow seam a caller (or test) uses to supply a fake backend
    /// without a live LLM call.
    #[must_use]
    #[cfg(feature = "rag-ner-llm")]
    pub fn with_backend(
        backend: Arc<dyn xberg::NerBackend>,
        categories: Vec<xberg::EntityCategory>,
    ) -> Self {
        Self {
            enabled: true,
            backend: Some(backend),
            categories,
        }
    }

    /// Enrichment enabled with an `xberg::LlmBackend` built from `config`,
    /// detecting the [`default_mythic_categories`] — the categories a mythic
    /// corpus actually needs, rather than the generic PERSON/ORG/LOCATION set.
    #[must_use]
    #[cfg(feature = "rag-ner-llm")]
    pub fn llm(config: xberg::LlmConfig) -> Self {
        let backend = Arc::new(xberg::LlmBackend::new(config)) as Arc<dyn xberg::NerBackend>;
        Self::with_backend(backend, default_mythic_categories())
    }
}

/// The mythic-corpus entity categories an [`EntityEnrichment::llm`] backend
/// detects by default: figures, protagonists, artifacts, and realms — the
/// entity shapes generation priors actually care about, distinct from the
/// generic PERSON/ORGANIZATION/LOCATION taxonomy built-in categories cover.
#[cfg(feature = "rag-ner-llm")]
fn default_mythic_categories() -> Vec<xberg::EntityCategory> {
    vec![
        xberg::EntityCategory::Custom(MYTHIC_CATEGORY_DEITY.to_owned()),
        xberg::EntityCategory::Custom(MYTHIC_CATEGORY_HERO.to_owned()),
        xberg::EntityCategory::Custom(MYTHIC_CATEGORY_ARTIFACT.to_owned()),
        xberg::EntityCategory::Custom(MYTHIC_CATEGORY_REALM.to_owned()),
    ]
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
    // `async` is genuinely needed: `xberg::detect_entities` is async and is
    // `.await`ed in the `rag-ner-llm` body below. With the feature off there is
    // no await in this stub body, so `clippy::unused_async` fires spuriously —
    // allow it only in that configuration rather than dropping `async` and
    // forcing every call site to branch on the feature.
    #[cfg_attr(not(feature = "rag-ner-llm"), allow(clippy::unused_async))]
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

        // Pre-embed the raw query for the dense arm (query-side prefix preserved
        // by `embed_query`); an FTS5-escaped term string drives the lexical arm.
        // An all-punctuation query escapes to no terms, degrading to vector-only.
        let query_vector = self
            .embedder
            .embed_query(vec![text.to_owned()])
            .await
            .map_err(|error| KnowledgeError::store("embedding reference query", error))?
            .pop();
        let escaped = fts5_match_query(text);

        // Reference-path keyword narrowing (WS-C slice 2): when enrichment is on,
        // bias candidates toward reference documents that share a salient query
        // term. A filter that starves retrieval — no document shares a keyword,
        // so the filtered fetch comes back empty — degrades to the unfiltered
        // path, guaranteeing the distiller is never left without grounding it
        // would otherwise have had. When enrichment is off (the default), the
        // filter is `None` and this is byte-for-byte the prior unfiltered path.
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

        // No lexical terms (empty/all-punctuation query) → plain vector.
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
mod tests {
    use crate::rag::InMemoryVectorStore;
    use crate::rag::RagResult;
    use async_trait::async_trait;

    use super::*;

    /// Deterministic fake embedder: a stable, content-derived vector of the
    /// collection dimension. No ONNX, no network.
    #[derive(Debug)]
    struct FakeEmbedder;

    #[async_trait]
    impl Embedder for FakeEmbedder {
        async fn embed(&self, texts: Vec<String>) -> RagResult<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|text| deterministic_vector(text))
                .collect())
        }
    }

    fn deterministic_vector(text: &str) -> Vec<f32> {
        let mut vector = vec![0.0f32; EMBEDDING_DIM as usize];
        for (index, byte) in text.bytes().enumerate() {
            let slot = index % EMBEDDING_DIM as usize;
            vector[slot] += f32::from(byte) / 255.0;
        }
        vector
    }

    fn test_knowledge() -> Knowledge {
        let store: Arc<dyn crate::rag::VectorStore> =
            Arc::new(InMemoryVectorStore::new(STORE_NAME));
        let embedder: Arc<dyn Embedder> = Arc::new(FakeEmbedder);
        let ledger = Ledger::load_embedded().expect("embedded manifest parses");
        Knowledge::with(store, embedder, ledger)
    }

    /// Deterministic fake NER backend: scans `text` for "hero" and "threshold"
    /// (both present in [`KEYWORD_TEST_CORPUS`]) and reports them at their real
    /// byte offsets. No ONNX, no network, no LLM call.
    #[cfg(feature = "rag-ner-llm")]
    #[derive(Debug)]
    struct FakeNerBackend;

    #[cfg(feature = "rag-ner-llm")]
    #[async_trait]
    impl xberg::NerBackend for FakeNerBackend {
        async fn detect(
            &self,
            text: &str,
            _categories: &[xberg::EntityCategory],
        ) -> xberg::Result<Vec<xberg::Entity>> {
            let mut entities = Vec::new();
            if let Some(start) = text.find("hero") {
                let start = u32::try_from(start).expect("test fixture offsets fit in u32");
                entities.push(xberg::Entity {
                    category: xberg::EntityCategory::Custom("Hero".to_owned()),
                    text: "hero".to_owned(),
                    start,
                    end: start + u32::try_from("hero".len()).expect("literal length fits in u32"),
                    confidence: Some(1.0),
                });
            }
            if let Some(start) = text.find("threshold") {
                let start = u32::try_from(start).expect("test fixture offsets fit in u32");
                entities.push(xberg::Entity {
                    category: xberg::EntityCategory::Custom("Realm".to_owned()),
                    text: "threshold".to_owned(),
                    start,
                    end: start
                        + u32::try_from("threshold".len()).expect("literal length fits in u32"),
                    confidence: Some(1.0),
                });
            }
            Ok(entities)
        }
    }

    #[tokio::test]
    async fn embed_texts_returns_one_vector_per_input_in_order() {
        let knowledge = test_knowledge();
        let vectors = knowledge
            .embed_texts(vec!["a hero departs".to_owned(), "the return".to_owned()])
            .await
            .expect("embedding succeeds");

        assert_eq!(vectors.len(), 2, "one vector per input");
        assert_eq!(vectors[0].len(), EMBEDDING_DIM as usize);
        assert_eq!(vectors[1].len(), EMBEDDING_DIM as usize);
        assert_ne!(
            vectors[0], vectors[1],
            "distinct inputs must embed to distinct vectors"
        );

        // Deterministic: the same input embeds to the same vector.
        let again = knowledge
            .embed_texts(vec!["a hero departs".to_owned()])
            .await
            .expect("embedding succeeds");
        assert_eq!(again[0], vectors[0], "embedding must be deterministic");
    }

    /// Build a retrieved chunk carrying `content` at `score`, for exercising the
    /// pure dedup helper without a live store.
    fn retrieved_chunk(content: &str, score: f32) -> RetrievedChunk {
        RetrievedChunk {
            id: crate::rag::ChunkId(format!("chunk-{score}")),
            document_id: DocumentId("doc".to_owned()),
            ordinal: 0,
            external_id: None,
            content: Some(content.to_owned()),
            score,
            primary_score: crate::rag::PrimaryScore::Vector(score),
            chunk_metadata: Value::Null,
            document: None,
        }
    }

    #[test]
    fn normalize_for_dedup_collapses_whitespace_and_case() {
        assert_eq!(
            normalize_for_dedup("The  Hero\n Returns"),
            normalize_for_dedup("the hero returns"),
            "case and whitespace differences must normalize to the same key"
        );
        assert_ne!(
            normalize_for_dedup("the hero returns"),
            normalize_for_dedup("the hero departs"),
            "genuinely different text must not collapse"
        );
    }

    #[test]
    fn dedup_chunks_drops_repeats_keeps_first_and_honors_limit() {
        // Highest-ranked first; the second "alpha" is a duplicate of the first.
        let chunks = vec![
            retrieved_chunk("Alpha passage.", 0.9),
            retrieved_chunk("Beta passage.", 0.8),
            retrieved_chunk("alpha   passage.", 0.7),
            retrieved_chunk("Gamma passage.", 0.6),
        ];

        let kept = dedup_chunks(chunks, 2);

        assert_eq!(kept.len(), 2, "limit is honored after dedup");
        assert_eq!(
            kept.iter()
                .map(|chunk| chunk.content.clone().unwrap_or_default())
                .collect::<Vec<_>>(),
            vec!["Alpha passage.".to_owned(), "Beta passage.".to_owned()],
            "the duplicate alpha is dropped and the highest-ranked copies are kept in order",
        );
    }

    #[tokio::test]
    async fn ingest_refuses_reference_namespace_source() {
        let knowledge = test_knowledge();
        let error = knowledge
            .ingest("perseus", IngestInput::new("some reference text"))
            .await
            .expect_err("reference-namespace source must be refused");
        match error {
            KnowledgeError::RefusedNonShip { id, namespace } => {
                assert_eq!(id, "perseus");
                assert_eq!(namespace, Namespace::Reference);
            }
            other => panic!("expected RefusedNonShip, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ingest_reference_refuses_ship_namespace_source() {
        let knowledge = test_knowledge();
        let error = knowledge
            .ingest_reference(
                "polti",
                IngestInput::new("ship text misrouted to reference ingest"),
            )
            .await
            .expect_err("a ship-namespace source must be refused by the reference gate");
        match error {
            KnowledgeError::RefusedNonReference { id, namespace } => {
                assert_eq!(id, "polti");
                assert_eq!(namespace, Namespace::Ship);
            }
            other => panic!("expected RefusedNonReference, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ingest_reference_rejects_undeclared_source() {
        let knowledge = test_knowledge();
        let error = knowledge
            .ingest_reference("not_a_real_source", IngestInput::new("text"))
            .await
            .expect_err("undeclared source must be rejected by the reference gate too");
        match error {
            KnowledgeError::UndeclaredSource { id } => assert_eq!(id, "not_a_real_source"),
            other => panic!("expected UndeclaredSource, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ingest_reference_then_reference_retrieve_round_trips_and_is_not_surfaceable() {
        let knowledge = test_knowledge();
        knowledge
            .ingest_reference(
                "perseus",
                IngestInput::new("The hero descends to the underworld and returns transformed."),
            )
            .await
            .expect("reference source ingests into the reference collection");

        let passages = knowledge
            .retrieve(KnowledgeQuery::reference("a descent to the underworld", 5))
            .await
            .expect("reference retrieval succeeds");

        assert!(
            !passages.is_empty(),
            "expected at least one reference passage"
        );
        let passage = &passages[0];
        assert_eq!(passage.namespace, Namespace::Reference);
        assert_eq!(passage.source_id, "perseus");
        assert!(
            !passage.is_surfaceable(),
            "a reference passage must never be surfaceable",
        );
    }

    /// Build a knowledge layer over a hybrid-capable in-memory **sqlite** store
    /// (sqlite-vec + FTS5), so the reference path actually exercises hybrid
    /// retrieval rather than falling back to vector on the in-memory store.
    async fn sqlite_test_knowledge() -> Knowledge {
        let store: Arc<dyn crate::rag::VectorStore> = Arc::new(
            SqliteVectorStore::open_in_memory(STORE_NAME)
                .await
                .expect("in-memory sqlite store opens"),
        );
        let embedder: Arc<dyn Embedder> = Arc::new(FakeEmbedder);
        let ledger = Ledger::load_embedded().expect("embedded manifest parses");
        Knowledge::with(store, embedder, ledger)
    }

    #[tokio::test]
    async fn reference_hybrid_retrieval_survives_punctuation_that_crashes_raw_fts5() {
        // This exact query — apostrophe, colon, comma — is a syntax error to
        // FTS5's MATCH parser if passed raw; the escaping + pre-embed path must
        // make it succeed rather than crash.
        let knowledge = sqlite_test_knowledge().await;
        knowledge
            .ingest_reference(
                "perseus",
                IngestInput::new(
                    "The hero's descent to the underworld: a trial, a nadir, and the return.",
                ),
            )
            .await
            .expect("reference source ingests into the reference collection");

        let passages = knowledge
            .retrieve(KnowledgeQuery::reference(
                "the hero's descent: trial, nadir, and return",
                5,
            ))
            .await
            .expect("hybrid reference retrieval must not crash on punctuation");

        assert!(
            !passages.is_empty(),
            "expected at least one reference passage"
        );
        assert!(
            passages.iter().all(|passage| !passage.is_surfaceable()),
            "a reference passage must never be surfaceable",
        );
        assert!(
            passages
                .iter()
                .all(|passage| passage.namespace == Namespace::Reference),
            "hybrid reference retrieval must stay in the reference namespace",
        );
    }

    /// The small ship corpus shared by the keyword- and entity-enrichment tests
    /// below, so both exercise the exact same ingest content.
    #[cfg(any(feature = "rag-keywords", feature = "rag-ner-llm"))]
    const KEYWORD_TEST_CORPUS: &str = "The hero departs the ordinary world, crosses the threshold into the \
         unknown, faces trials with the aid of allies and mentors, and returns \
         transformed, bringing the boon home to the ordinary world.";

    /// WS-C #49: populating `doc.keywords` at ingest must never perturb the
    /// surfaceable (ship) retrieval path — it stays on plain vector search,
    /// unfiltered and unordered by keyword overlap. Ingest the identical ship
    /// corpus into two stores, one with keyword enrichment enabled and one
    /// disabled, run the identical surfaceable query against both, and assert
    /// the returned passages serialize to byte-identical JSON.
    #[tokio::test]
    #[cfg(feature = "rag-keywords")]
    async fn keyword_enrichment_does_not_perturb_surfaceable_retrieval() {
        let enriched = test_knowledge().with_keyword_enrichment(KeywordEnrichment::enabled());
        let plain = test_knowledge();

        for knowledge in [&enriched, &plain] {
            knowledge
                .ingest("polti", IngestInput::new(KEYWORD_TEST_CORPUS))
                .await
                .expect("ship source ingests");
        }

        let query = || KnowledgeQuery::surfaceable("a hero returns transformed", 5);
        let enriched_passages = enriched
            .retrieve(query())
            .await
            .expect("surfaceable retrieval succeeds against the enriched store");
        let plain_passages = plain
            .retrieve(query())
            .await
            .expect("surfaceable retrieval succeeds against the plain store");

        let enriched_json =
            serde_json::to_string(&enriched_passages).expect("passages serialize to JSON");
        let plain_json =
            serde_json::to_string(&plain_passages).expect("passages serialize to JSON");

        assert_eq!(
            enriched_json, plain_json,
            "keyword enrichment at ingest must not perturb the surfaceable retrieval path: \
             the ship query must return byte-identical passages whether or not \
             `doc.keywords` was populated during ingest"
        );
    }

    /// Companion to the byte-identity guard above: proves enrichment is not a
    /// silent no-op. Without this assertion, the byte-identity test could pass
    /// vacuously if keyword extraction always produced an empty vector (e.g. a
    /// wiring bug that never calls `extract_keywords` at all). Ingests the same
    /// corpus on the *reference* path (never surfaced, so this is purely an
    /// internal check) with enrichment enabled and inspects the stored
    /// document's `keywords` via a direct reference-collection retrieve, whose
    /// `RetrievedChunk::document` carries the full `DocumentRecord` (including
    /// `keywords`) when `include_document` is set.
    #[tokio::test]
    #[cfg(feature = "rag-keywords")]
    async fn keyword_enrichment_populates_keywords_when_enabled() {
        let knowledge = test_knowledge().with_keyword_enrichment(KeywordEnrichment::enabled());
        knowledge
            .ingest_reference("perseus", IngestInput::new(KEYWORD_TEST_CORPUS))
            .await
            .expect("reference source ingests into the reference collection");

        let query = RetrieveQuery {
            query_text: Some("a hero returns transformed".to_owned()),
            include_content: true,
            include_document: true,
            ..RetrieveQuery::vector(5)
        };
        let chunks = knowledge
            .run_retrieve(REFERENCE_COLLECTION, query)
            .await
            .expect("direct reference retrieval succeeds");

        assert!(!chunks.is_empty(), "expected at least one retrieved chunk");
        let keywords = chunks
            .into_iter()
            .find_map(|chunk| chunk.document.map(|document| document.keywords))
            .expect("retrieved chunk must carry its parent document with keywords");

        assert!(
            !keywords.is_empty(),
            "keyword enrichment must actually populate `doc.keywords` when enabled, \
             not silently no-op"
        );
    }

    /// WS-C slice 2: with enrichment disabled (the default), the reference path
    /// builds no keyword filter, so reference retrieval is byte-for-byte the
    /// prior unfiltered behavior. This holds regardless of the `rag-keywords`
    /// feature, so it is deliberately not feature-gated.
    #[test]
    fn reference_keyword_filter_is_none_when_enrichment_disabled() {
        let knowledge = test_knowledge();
        assert!(
            knowledge
                .reference_keyword_filter("the hero crosses the threshold")
                .is_none(),
            "a store with enrichment disabled must never narrow the reference path"
        );
    }

    /// WS-C slice 2: with enrichment enabled, a keyword-bearing query yields a
    /// disjunction of `ArrayContains` predicates, each targeting `doc.keywords`,
    /// so a reference document is a candidate when it shares *any* salient term.
    #[test]
    #[cfg(feature = "rag-keywords")]
    fn reference_keyword_filter_builds_array_contains_disjunction_over_doc_keywords() {
        let knowledge = test_knowledge().with_keyword_enrichment(KeywordEnrichment::enabled());
        let filter = knowledge
            .reference_keyword_filter(KEYWORD_TEST_CORPUS)
            .expect("a keyword-bearing query must produce a filter when enrichment is enabled");

        // A single salient term degenerates to a bare `ArrayContains`; several
        // combine under `Or`. Either way every leaf targets `doc.keywords`.
        let predicates = match &filter {
            Filter::Or { filters } => filters.clone(),
            single @ Filter::ArrayContains { .. } => vec![single.clone()],
            other => panic!("expected an ArrayContains disjunction, got {other:?}"),
        };
        assert!(
            predicates.len() <= MAX_QUERY_KEYWORD_PREDICATES,
            "predicate count must be capped at MAX_QUERY_KEYWORD_PREDICATES"
        );
        for predicate in &predicates {
            match predicate {
                Filter::ArrayContains { field, value } => {
                    assert_eq!(
                        field.0, DOC_KEYWORDS_FIELD,
                        "every keyword predicate must target the doc.keywords field"
                    );
                    assert!(value.is_string(), "each predicate matches a keyword string");
                }
                other => panic!("every predicate must be ArrayContains, got {other:?}"),
            }
        }
        // The filter must satisfy the IR complexity caps so retrieval accepts it.
        filter
            .validate()
            .expect("keyword filter must be within complexity caps");
    }

    /// WS-C slice 2 (the load-bearing behavioral guarantee): a keyword filter
    /// that no reference document satisfies must **degrade to the unfiltered
    /// path** rather than starve the distiller. Ingest one reference document,
    /// then query with vocabulary disjoint from it: a filter *is* built (proving
    /// the narrowing was attempted) yet retrieval still returns the document,
    /// which is only possible via the unfiltered fallback.
    #[tokio::test]
    #[cfg(feature = "rag-keywords")]
    async fn reference_retrieval_degrades_to_unfiltered_when_no_document_shares_a_keyword() {
        let knowledge = test_knowledge().with_keyword_enrichment(KeywordEnrichment::enabled());
        knowledge
            .ingest_reference("perseus", IngestInput::new(KEYWORD_TEST_CORPUS))
            .await
            .expect("reference source ingests");

        // Vocabulary deliberately disjoint from the hero's-journey corpus, so no
        // stored keyword can match — the filtered fetch is empty.
        let disjoint_query = "photolithography semiconductor wafer fabrication throughput";
        assert!(
            knowledge.reference_keyword_filter(disjoint_query).is_some(),
            "the disjoint query must still produce keywords (so a filter is built)"
        );

        let passages = knowledge
            .retrieve(KnowledgeQuery::reference(disjoint_query, 5))
            .await
            .expect("reference retrieval succeeds");

        assert!(
            !passages.is_empty(),
            "a non-matching keyword filter must degrade to unfiltered retrieval, \
             not return an empty grounding set"
        );
        assert!(
            passages.iter().all(|passage| !passage.is_surfaceable()),
            "reference passages are never surfaceable regardless of enrichment"
        );
    }

    /// WS-C slice 2: the filtered (non-empty) path returns grounding that shares
    /// a query keyword. Querying with the ingested corpus's own text extracts the
    /// same keywords stored on the document, so the `ArrayContains` disjunction
    /// matches exactly and the document is returned *through* the filter — not
    /// via the fallback.
    #[tokio::test]
    #[cfg(feature = "rag-keywords")]
    async fn reference_retrieval_with_enrichment_returns_grounding_that_shares_a_keyword() {
        let knowledge = test_knowledge().with_keyword_enrichment(KeywordEnrichment::enabled());
        knowledge
            .ingest_reference("perseus", IngestInput::new(KEYWORD_TEST_CORPUS))
            .await
            .expect("reference source ingests");

        let passages = knowledge
            .retrieve(KnowledgeQuery::reference(KEYWORD_TEST_CORPUS, 5))
            .await
            .expect("reference retrieval succeeds");

        assert!(
            !passages.is_empty(),
            "a query sharing keywords with the ingested document must return it \
             through the keyword filter"
        );
        assert_eq!(
            passages[0].source_id, "perseus",
            "the keyword-matching reference document is the grounding returned"
        );
    }

    /// WS-C slice 3, the entity-enrichment analogue of
    /// `keyword_enrichment_does_not_perturb_surfaceable_retrieval`: populating
    /// `doc.entities` at ingest must never perturb the surfaceable (ship)
    /// retrieval path. Ingest the identical ship corpus into two stores, one
    /// with entity enrichment enabled and one disabled, run the identical
    /// surfaceable query against both, and assert the returned passages
    /// serialize to byte-identical JSON.
    #[tokio::test]
    #[cfg(feature = "rag-ner-llm")]
    async fn entity_enrichment_does_not_perturb_surfaceable_retrieval() {
        let enriched = test_knowledge().with_entity_enrichment(EntityEnrichment::with_backend(
            Arc::new(FakeNerBackend),
            vec![
                xberg::EntityCategory::Custom("Hero".to_owned()),
                xberg::EntityCategory::Custom("Realm".to_owned()),
            ],
        ));
        let plain = test_knowledge();

        for knowledge in [&enriched, &plain] {
            knowledge
                .ingest("polti", IngestInput::new(KEYWORD_TEST_CORPUS))
                .await
                .expect("ship source ingests");
        }

        let query = || KnowledgeQuery::surfaceable("a hero returns transformed", 5);
        let enriched_passages = enriched
            .retrieve(query())
            .await
            .expect("surfaceable retrieval succeeds against the enriched store");
        let plain_passages = plain
            .retrieve(query())
            .await
            .expect("surfaceable retrieval succeeds against the plain store");

        let enriched_json =
            serde_json::to_string(&enriched_passages).expect("passages serialize to JSON");
        let plain_json =
            serde_json::to_string(&plain_passages).expect("passages serialize to JSON");

        assert_eq!(
            enriched_json, plain_json,
            "entity enrichment at ingest must not perturb the surfaceable retrieval path: \
             the ship query must return byte-identical passages whether or not \
             `doc.entities` was populated during ingest"
        );
    }

    /// Companion to the byte-identity guard above: proves entity enrichment is
    /// not a silent no-op. Without this assertion, the byte-identity test could
    /// pass vacuously if entity detection always produced an empty vector (e.g.
    /// a wiring bug that never calls `detect_entities` at all). Ingests the same
    /// corpus on the *reference* path (never surfaced, so this is purely an
    /// internal check) with enrichment enabled and inspects the stored
    /// document's `entities` via a direct reference-collection retrieve, whose
    /// `RetrievedChunk::document` carries the full `DocumentRecord` (including
    /// `entities`) when `include_document` is set.
    #[tokio::test]
    #[cfg(feature = "rag-ner-llm")]
    async fn entity_enrichment_populates_entities_when_enabled() {
        let knowledge = test_knowledge().with_entity_enrichment(EntityEnrichment::with_backend(
            Arc::new(FakeNerBackend),
            vec![
                xberg::EntityCategory::Custom("Hero".to_owned()),
                xberg::EntityCategory::Custom("Realm".to_owned()),
            ],
        ));
        knowledge
            .ingest_reference("perseus", IngestInput::new(KEYWORD_TEST_CORPUS))
            .await
            .expect("reference source ingests into the reference collection");

        let query = RetrieveQuery {
            query_text: Some("a hero returns transformed".to_owned()),
            include_content: true,
            include_document: true,
            ..RetrieveQuery::vector(5)
        };
        let chunks = knowledge
            .run_retrieve(REFERENCE_COLLECTION, query)
            .await
            .expect("direct reference retrieval succeeds");

        let entities = chunks
            .into_iter()
            .find_map(|chunk| chunk.document.map(|document| document.entities))
            .expect("retrieved chunk must carry its parent document with entities");

        let entities = entities.as_array().expect("entities must be a JSON array");
        assert!(
            !entities.is_empty(),
            "entity enrichment must actually populate `doc.entities` when enabled, \
             not silently no-op"
        );
        assert!(
            entities.iter().all(serde_json::Value::is_string),
            "each stored entity must be a surface-string, not a nested object"
        );
    }

    /// Capability proof: a reference document is retrievable by a
    /// `Filter::ArrayContains` predicate over `doc.entities`, end to end — the
    /// detected entity actually lands somewhere the filter IR can address, not
    /// just somewhere `include_document` happens to expose.
    #[tokio::test]
    #[cfg(feature = "rag-ner-llm")]
    async fn reference_document_is_retrievable_by_a_detected_entity_via_array_contains() {
        let knowledge = test_knowledge().with_entity_enrichment(EntityEnrichment::with_backend(
            Arc::new(FakeNerBackend),
            vec![
                xberg::EntityCategory::Custom("Hero".to_owned()),
                xberg::EntityCategory::Custom("Realm".to_owned()),
            ],
        ));
        knowledge
            .ingest_reference("perseus", IngestInput::new(KEYWORD_TEST_CORPUS))
            .await
            .expect("reference source ingests into the reference collection");

        let query = RetrieveQuery {
            query_text: Some("a hero returns transformed".to_owned()),
            filter: Some(Filter::ArrayContains {
                field: FilterField("doc.entities".to_owned()),
                value: Value::String("hero".to_owned()),
            }),
            include_document: true,
            ..RetrieveQuery::vector(5)
        };
        let chunks = knowledge
            .run_retrieve(REFERENCE_COLLECTION, query)
            .await
            .expect("array-contains filtered retrieval succeeds");

        assert!(
            !chunks.is_empty(),
            "a document carrying the detected entity must be retrievable via \
             ArrayContains over doc.entities"
        );
    }

    #[test]
    fn fts5_match_query_quotes_each_token_neutralizing_operators() {
        // Apostrophe survives inside the quoted token; colon/comma no longer parse
        // as FTS5 operators.
        assert_eq!(
            fts5_match_query("the hero's descent: trial, return").as_deref(),
            Some("\"the\" \"hero's\" \"descent:\" \"trial,\" \"return\""),
        );
    }

    #[test]
    fn fts5_match_query_escapes_embedded_double_quotes() {
        assert_eq!(
            fts5_match_query("say \"hi\"").as_deref(),
            Some("\"say\" \"\"\"hi\"\"\""),
        );
    }

    #[test]
    fn fts5_match_query_returns_none_for_termless_input() {
        assert_eq!(fts5_match_query(""), None);
        assert_eq!(fts5_match_query("   \t\n  "), None);
    }

    /// ADR-0016 confirmation: a document ingested through the reference path is
    /// unreachable via the ship-gated (surfaceable) retrieval path — it lives in
    /// the reference collection, which a surfaceable query never targets, so it
    /// can never be surfaced verbatim.
    #[tokio::test]
    async fn reference_ingested_doc_is_unreachable_via_the_surfaceable_query() {
        let knowledge = test_knowledge();
        knowledge
            .ingest_reference(
                "perseus",
                IngestInput::new("The Suppliant implores a Power in authority for mercy and aid."),
            )
            .await
            .expect("reference source ingests into the reference collection");

        let passages = knowledge
            .retrieve(KnowledgeQuery::surfaceable("a plea for mercy and aid", 5))
            .await
            .expect("surfaceable retrieval succeeds");

        assert!(
            passages.iter().all(Passage::is_surfaceable),
            "a surfaceable query must never return a non-surfaceable passage",
        );
        assert!(
            passages
                .iter()
                .all(|passage| passage.source_id != "perseus"),
            "the reference-ingested doc must be unreachable through the ship-gated path",
        );
    }

    #[tokio::test]
    async fn ingest_rejects_undeclared_source() {
        let knowledge = test_knowledge();
        let error = knowledge
            .ingest("not_a_real_source", IngestInput::new("text"))
            .await
            .expect_err("undeclared source must be rejected");
        match error {
            KnowledgeError::UndeclaredSource { id } => assert_eq!(id, "not_a_real_source"),
            other => panic!("expected UndeclaredSource, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ingest_then_surfaceable_retrieve_round_trips_ship_source() {
        let knowledge = test_knowledge();
        let input = IngestInput {
            source_uri: Some("https://example.org/polti.txt".to_owned()),
            checksum: Some("sha256:deadbeef".to_owned()),
            retrieved: Some("2026-07-11".to_owned()),
            ..IngestInput::new("The Suppliant implores a Power in authority for mercy and aid.")
        };
        knowledge
            .ingest("polti", input)
            .await
            .expect("ship source ingests");

        let passages = knowledge
            .retrieve(KnowledgeQuery::surfaceable(
                "a plea to a powerful protector",
                5,
            ))
            .await
            .expect("retrieval succeeds");

        assert!(!passages.is_empty(), "expected at least one passage");
        let passage = &passages[0];
        assert_eq!(passage.namespace, Namespace::Ship);
        assert_eq!(passage.source_id, "polti");
        assert_eq!(
            passage.url.as_deref(),
            Some("https://example.org/polti.txt"),
            "provenance url must round-trip through stored metadata"
        );
        assert_eq!(
            passage.checksum.as_deref(),
            Some("sha256:deadbeef"),
            "provenance checksum must round-trip through stored metadata"
        );
        assert_eq!(
            passage.retrieved.as_deref(),
            Some("2026-07-11"),
            "provenance retrieved date must round-trip through stored metadata"
        );
    }

    #[tokio::test]
    async fn surfaceable_retrieve_never_returns_a_reference_tagged_doc_from_the_ship_collection() {
        let knowledge = test_knowledge();
        knowledge
            .ensure_collection(SHIP_COLLECTION)
            .await
            .expect("ship collection ensured");

        let metadata = serde_json::json!({
            META_SOURCE_ID: "smuggled",
            META_NAMESPACE: "reference",
            META_LICENSE: "in-copyright",
            META_TIER: "reference",
            META_DOMAIN: "myth",
        });
        let request = IngestRequest {
            full_text: "The Suppliant implores a Power in authority for mercy and aid.".to_owned(),
            metadata,
            ..IngestRequest::default()
        };
        let config = RagPipelineConfig {
            chunking: &knowledge.chunking,
        };
        ingest_document(
            Arc::clone(&knowledge.store),
            SHIP_COLLECTION,
            request,
            &config,
            knowledge.embedder.as_ref(),
        )
        .await
        .expect("direct write into the ship collection (bypassing the gate) succeeds");

        match knowledge
            .retrieve(KnowledgeQuery::surfaceable("a plea for mercy and aid", 5))
            .await
        {
            Ok(passages) => assert!(
                passages
                    .iter()
                    .all(|passage| passage.namespace == Namespace::Ship),
                "the ship filter must exclude the reference-tagged document",
            ),
            Err(KnowledgeError::NamespaceViolation { found, .. }) => {
                assert_eq!(
                    found,
                    Namespace::Reference,
                    "layer 3 caught what layer 2 let through",
                );
            }
            Err(other) => panic!("unexpected retrieval error: {other:?}"),
        }
    }

    #[test]
    fn ship_filter_targets_document_namespace_metadata() {
        let filter = ship_filter();
        match filter {
            Filter::Eq { field, value } => {
                assert_eq!(field.0, "doc.metadata.namespace");
                assert_eq!(value, Value::String("ship".to_owned()));
            }
            other => panic!("expected namespace equality filter, got {other:?}"),
        }
    }

    fn tagged_metadata(namespace: &str) -> Value {
        serde_json::json!({
            META_SOURCE_ID: "somesource",
            META_NAMESPACE: namespace,
            META_LICENSE: "some-license",
            META_DOMAIN: "myth",
        })
    }

    #[test]
    fn build_passage_refuses_non_ship_tag_in_ship_collection() {
        let metadata = tagged_metadata("reference");
        let error = build_passage(&metadata, Some("text".to_owned()), 1.0, SHIP_COLLECTION)
            .expect_err("a non-ship tag in the ship collection must be refused");
        match error {
            KnowledgeError::NamespaceViolation { found, .. } => {
                assert_eq!(found, Namespace::Reference);
            }
            other => panic!("expected NamespaceViolation, got {other:?}"),
        }
    }

    #[test]
    fn build_passage_refuses_ship_tag_in_reference_collection() {
        let metadata = tagged_metadata("ship");
        let error = build_passage(
            &metadata,
            Some("text".to_owned()),
            1.0,
            REFERENCE_COLLECTION,
        )
        .expect_err("a ship tag in the reference collection must be refused");
        match error {
            KnowledgeError::NamespaceViolation { found, .. } => {
                assert_eq!(found, Namespace::Ship);
            }
            other => panic!("expected NamespaceViolation, got {other:?}"),
        }
    }

    #[test]
    fn build_passage_refuses_empty_content() {
        let metadata = tagged_metadata("ship");
        let error = build_passage(&metadata, None, 1.0, SHIP_COLLECTION)
            .expect_err("missing content must be refused, not defaulted to empty");
        assert!(matches!(error, KnowledgeError::EmptyContent { .. }));
    }

    #[test]
    fn build_passage_accepts_a_well_formed_ship_chunk() {
        let metadata = tagged_metadata("ship");
        let passage = build_passage(
            &metadata,
            Some("the trial".to_owned()),
            0.5,
            SHIP_COLLECTION,
        )
        .expect("a well-formed ship chunk builds a passage");
        assert_eq!(passage.namespace, Namespace::Ship);
        assert_eq!(passage.source_id, "somesource");
        assert_eq!(passage.text, "the trial");
        assert_eq!(
            passage.url, None,
            "metadata without a url key must build a passage with no url"
        );
        assert_eq!(
            passage.checksum, None,
            "metadata without a checksum key must build a passage with no checksum"
        );
        assert_eq!(
            passage.retrieved, None,
            "metadata without a retrieved key must build a passage with no retrieved date"
        );
    }

    #[test]
    fn build_passage_reads_provenance_when_present_in_metadata() {
        let mut metadata = tagged_metadata("ship");
        metadata[META_URL] = Value::String("https://example.org/src.txt".to_owned());
        metadata[META_CHECKSUM] = Value::String("sha256:abc123".to_owned());
        metadata[META_RETRIEVED] = Value::String("2026-01-15".to_owned());

        let passage = build_passage(
            &metadata,
            Some("the trial".to_owned()),
            0.5,
            SHIP_COLLECTION,
        )
        .expect("a well-formed ship chunk with provenance builds a passage");
        assert_eq!(passage.url.as_deref(), Some("https://example.org/src.txt"));
        assert_eq!(passage.checksum.as_deref(), Some("sha256:abc123"));
        assert_eq!(passage.retrieved.as_deref(), Some("2026-01-15"));
    }

    #[test]
    fn ingest_metadata_omits_absent_provenance_keys() {
        let entry = SourceEntry {
            id: "somesource".to_owned(),
            name: "Some Source".to_owned(),
            tier: Tier::System,
            namespace: Namespace::Ship,
            domain: "myth".to_owned(),
            license: "CC0".to_owned(),
            note: None,
            url: None,
        };
        let metadata = ingest_metadata("somesource", &entry, &IngestInput::new("text"));

        let object = metadata.as_object().expect("metadata is a json object");
        assert!(
            !object.contains_key(META_URL),
            "no url given: the url key must be absent, not null"
        );
        assert!(
            !object.contains_key(META_CHECKSUM),
            "no checksum given: the checksum key must be absent, not null"
        );
        assert!(
            !object.contains_key(META_RETRIEVED),
            "no retrieved date given: the retrieved key must be absent, not null"
        );
    }

    #[test]
    fn ingest_metadata_includes_provenance_keys_when_present() {
        let entry = SourceEntry {
            id: "somesource".to_owned(),
            name: "Some Source".to_owned(),
            tier: Tier::System,
            namespace: Namespace::Ship,
            domain: "myth".to_owned(),
            license: "CC0".to_owned(),
            note: None,
            url: None,
        };
        let input = IngestInput {
            source_uri: Some("https://example.org/src.txt".to_owned()),
            checksum: Some("sha256:abc123".to_owned()),
            retrieved: Some("2026-01-15".to_owned()),
            ..IngestInput::new("text")
        };
        let metadata = ingest_metadata("somesource", &entry, &input);

        assert_eq!(
            metadata[META_URL],
            Value::String("https://example.org/src.txt".to_owned())
        );
        assert_eq!(
            metadata[META_CHECKSUM],
            Value::String("sha256:abc123".to_owned())
        );
        assert_eq!(
            metadata[META_RETRIEVED],
            Value::String("2026-01-15".to_owned())
        );
    }
}
