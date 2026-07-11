//! Ship-gated RAG over `xberg-rag`: the license ledger, ship-gated ingestion, and
//! ship-filtered retrieval.
//!
//! This crate owns the vector store, the embedded license ledger
//! ([`Ledger`], from `corpus/manifest.json`), and the two operations that must
//! enforce the hard commercial licensing invariant:
//!
//! - **Ingestion is ship-gated.** [`Knowledge::ingest`] admits only sources whose
//!   ledger [`Namespace`] is [`Namespace::Ship`] into the surfaceable store; any
//!   `reference` source is refused with [`KnowledgeError::RefusedNonShip`], and an
//!   unknown source id with [`KnowledgeError::UndeclaredSource`]. Nothing outside
//!   the `ship` namespace can ever enter the shippable collection.
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

#![forbid(unsafe_code)]

#[cfg(feature = "acquire")]
pub mod acquire;
mod audit;
mod error;
mod ledger;

use std::fmt;
use std::path::Path;
use std::sync::Arc;

use serde_json::Value;
use xberg::ChunkingConfig;
use xberg_rag::backends::sqlite::SqliteVectorStore;
use xberg_rag::pipeline::{
    CoreEmbedder, Embedder, IngestRequest, RagPipelineConfig, ingest_document,
    retrieve as pipeline_retrieve,
};
use xberg_rag::{CollectionSpec, DocumentId, Filter, FilterField, RetrieveQuery, RetrievedChunk};

#[cfg(feature = "acquire")]
pub use crate::acquire::{BuildOptions, BuildReport, SourceOutcome, SourceReport, build_corpus};
pub use crate::error::KnowledgeError;
pub use crate::ledger::{
    Ledger, Namespace, REFERENCE_COLLECTION, SHIP_COLLECTION, SourceEntry, Tier,
};

/// Embedding dimension of the default (`balanced`) `CoreEmbedder` preset.
///
/// Collections are created at this dimension; a test embedder must emit vectors
/// of this width to match.
pub const EMBEDDING_DIM: u32 = 768;

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
    /// Note: [`Knowledge::ingest`] is ship-only today, so the reference
    /// collection is unpopulated and this returns no passages until a
    /// reference-ingest path is wired up.
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
#[derive(Debug, Clone, PartialEq)]
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
    store: Arc<dyn xberg_rag::VectorStore>,
    embedder: Arc<dyn Embedder>,
    ledger: Ledger,
    chunking: ChunkingConfig,
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
        store: Arc<dyn xberg_rag::VectorStore>,
        embedder: Arc<dyn Embedder>,
        ledger: Ledger,
    ) -> Self {
        Self {
            store,
            embedder,
            ledger,
            chunking: ChunkingConfig::default(),
        }
    }

    /// Ingest `input` under the ledger-declared source `source_id`.
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
        let entry = self
            .ledger
            .get(source_id)
            .ok_or_else(|| KnowledgeError::UndeclaredSource {
                id: source_id.to_owned(),
            })?;

        if entry.namespace != Namespace::Ship {
            return Err(KnowledgeError::RefusedNonShip {
                id: source_id.to_owned(),
                namespace: entry.namespace,
            });
        }

        let metadata = ingest_metadata(source_id, entry, &input);

        let request = IngestRequest {
            full_text: input.full_text,
            title: input.title,
            source_uri: input.source_uri,
            metadata,
            ..IngestRequest::default()
        };

        self.ensure_collection(SHIP_COLLECTION).await?;
        let config = RagPipelineConfig {
            chunking: &self.chunking,
        };
        ingest_document(
            Arc::clone(&self.store),
            SHIP_COLLECTION,
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
        let (collection, filter) = if query.surfaceable {
            (SHIP_COLLECTION, Some(ship_filter()))
        } else {
            (REFERENCE_COLLECTION, None)
        };

        self.ensure_collection(collection).await?;

        let retrieve_query = RetrieveQuery {
            query_text: Some(query.text),
            filter,
            include_content: true,
            include_document: true,
            ..RetrieveQuery::vector(query.top_k)
        };

        let chunks = pipeline_retrieve(
            Arc::clone(&self.store),
            collection,
            retrieve_query,
            Some(self.embedder.as_ref()),
        )
        .await
        .map_err(|error| KnowledgeError::store("retrieving chunks", error))?;

        chunks
            .into_iter()
            .map(|chunk| passage_from_chunk(chunk, collection))
            .collect()
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

/// Build a [`Passage`] from a retrieved chunk, reading licensing provenance from
/// the parent document's metadata.
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
    use async_trait::async_trait;
    use xberg_rag::InMemoryVectorStore;
    use xberg_rag::RagResult;

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
        let store: Arc<dyn xberg_rag::VectorStore> = Arc::new(InMemoryVectorStore::new(STORE_NAME));
        let embedder: Arc<dyn Embedder> = Arc::new(FakeEmbedder);
        let ledger = Ledger::load_embedded().expect("embedded manifest parses");
        Knowledge::with(store, embedder, ledger)
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
