//! Offline integration tests for [`draft_law`], exercising the full
//! retrieve -> distill -> gate -> stamp pipeline against an in-memory
//! [`Knowledge`] and a canned [`StructuredBackend`]. No network calls.

use std::sync::Arc;

use async_trait::async_trait;
use monomyth_frameworks::load_law;
use monomyth_knowledge::{EMBEDDING_DIM, IngestInput, Knowledge, KnowledgeQuery, Ledger};
use monomyth_llm::{BackendError, Llm, StructuredBackend, Usage};
use monomyth_synthesis::{DraftRequest, SynthesisError, draft_law};
use serde_json::{Value, json};
use xberg_rag::pipeline::Embedder;
use xberg_rag::{InMemoryVectorStore, RagResult};

/// A reference-namespace source declared in the embedded corpus ledger, used
/// to ingest self-authored synthetic text (never copyrighted bytes) under a
/// real reference source id.
const REFERENCE_SOURCE_ID: &str = "perseus";

/// Self-authored synthetic reference text (owned by us, not copied from
/// Perseus or any other corpus source) — long enough to carry an unambiguous
/// 8-word run for the verbatim-leak test.
const SYNTHETIC_REFERENCE_TEXT: &str = "The Suppliant implores a Power in authority to grant a boon of mercy, and the mediator \
     stands between two poles until the trial is resolved.";

/// A deterministic, content-derived embedder of the collection dimension — no
/// ONNX, no network. Mirrors the pattern in `monomyth-gen`'s test support.
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

/// A deterministic, content-derived embedding vector: no ONNX, no network.
fn deterministic_vector(text: &str) -> Vec<f32> {
    let width = EMBEDDING_DIM as usize;
    let mut vector = vec![0.0f32; width];
    for (index, byte) in text.bytes().enumerate() {
        vector[index % width] += f32::from(byte) / 255.0;
    }
    vector
}

/// A knowledge layer over an in-memory store and the fake embedder, with
/// nothing ingested.
fn test_knowledge() -> Knowledge {
    let store: Arc<dyn xberg_rag::VectorStore> =
        Arc::new(InMemoryVectorStore::new("synthesis-test"));
    let embedder: Arc<dyn Embedder> = Arc::new(FakeEmbedder);
    let ledger = Ledger::load_embedded().expect("embedded manifest parses");
    Knowledge::with(store, embedder, ledger)
}

/// A knowledge layer with [`SYNTHETIC_REFERENCE_TEXT`] ingested under
/// [`REFERENCE_SOURCE_ID`] via the reference-only ingest path.
async fn knowledge_with_reference_text() -> Knowledge {
    let knowledge = test_knowledge();
    knowledge
        .ingest_reference(
            REFERENCE_SOURCE_ID,
            IngestInput::new(SYNTHETIC_REFERENCE_TEXT),
        )
        .await
        .expect("self-authored synthetic text ingests under a reference source id");
    knowledge
}

/// A canned [`StructuredBackend`] that returns a fixed JSON response for
/// `complete_json`, regardless of prompt or schema name. This injects the
/// transport seam at the crate boundary (`StructuredBackend` is public) — it
/// is not mocking an internal service.
struct CannedBackend {
    response: Value,
}

impl CannedBackend {
    fn new(response: Value) -> Self {
        Self { response }
    }
}

#[async_trait]
impl StructuredBackend for CannedBackend {
    async fn complete_json(
        &self,
        _prompt: &str,
        _schema_name: &str,
        _schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError> {
        Ok((
            self.response.clone(),
            Some(Usage {
                prompt_tokens: Some(100),
                completion_tokens: Some(50),
                total_tokens: Some(150),
            }),
        ))
    }

    async fn complete_text(&self, _prompt: &str) -> Result<(String, Option<Usage>), BackendError> {
        Err(BackendError::new(
            "text completion is not used by draft_law",
        ))
    }
}

/// A clean paraphrase of [`SYNTHETIC_REFERENCE_TEXT`]'s idea, sharing no
/// 8-word run with it, so the anti-leak gate passes it.
fn clean_candidate_json() -> Value {
    json!({
        "title": "The Mediating Plea",
        "items": [
            {
                "name": "Petition",
                "description": "A dependent figure appeals to a higher authority for aid it cannot compel."
            },
            {
                "name": "Threshold Stand",
                "description": "An intermediary occupies the space between two opposing forces until resolution."
            }
        ]
    })
}

/// A candidate embedding a verbatim >=8-word span from
/// [`SYNTHETIC_REFERENCE_TEXT`] ("the suppliant implores a power in authority
/// to grant"), which must trip the anti-leak gate.
fn leaking_candidate_json() -> Value {
    json!({
        "title": "The Mediating Plea",
        "items": [
            {
                "name": "Petition",
                "description": "In this taxonomy, the suppliant implores a power in authority to grant relief."
            },
            {
                "name": "Threshold Stand",
                "description": "An intermediary occupies the space between two opposing forces until resolution."
            }
        ]
    })
}

fn base_request() -> DraftRequest {
    DraftRequest {
        law_id: "mediating_plea".to_owned(),
        domain: "myth".to_owned(),
        query: "a plea to a powerful protector".to_owned(),
        top_k: 5,
        model: "anthropic/claude-sonnet-4-20250514".to_owned(),
        generated: "2026-07-11".to_owned(),
    }
}

#[tokio::test]
async fn draft_law_happy_path_produces_a_pre_review_artifact_that_load_law_rejects_until_reviewed()
{
    let knowledge = knowledge_with_reference_text().await;
    let llm = Llm::new(Box::new(CannedBackend::new(clean_candidate_json())));
    let request = base_request();

    let drafted = draft_law(&knowledge, &llm, &request)
        .await
        .expect("a clean paraphrase candidate must draft successfully");

    let artifact = &drafted.artifact;
    assert_eq!(artifact.law, "mediating_plea");
    assert_eq!(artifact.namespace, "ship");
    assert_eq!(artifact.tier, "system");
    assert_eq!(artifact.domain, "myth");
    assert_eq!(artifact.title, "The Mediating Plea");
    assert_eq!(artifact.count, 2);
    assert_eq!(
        artifact
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>(),
        vec![1, 2],
        "item ids must be contiguous 1..=count"
    );
    assert_eq!(artifact.items[0].name, "Petition");
    assert_eq!(
        artifact.items[1].description,
        "An intermediary occupies the space between two opposing forces until resolution."
    );
    assert_eq!(
        artifact.synthesis.reviewed_by, "",
        "a freshly drafted artifact must have an empty reviewer"
    );
    assert!(
        artifact.synthesis.candidate_sha256.is_some(),
        "the candidate hash must be recorded for audit"
    );
    assert_eq!(
        artifact.synthesis.reference_source_ids,
        vec![REFERENCE_SOURCE_ID.to_owned()],
        "must name the ingested reference source"
    );
    assert_eq!(artifact.synthesis.model, request.model);
    assert_eq!(artifact.synthesis.generated, request.generated);

    assert_eq!(drafted.passages.len(), 1);
    assert_eq!(drafted.passages[0].source_id, REFERENCE_SOURCE_ID);
    assert!(
        !drafted.passages[0].is_surfaceable(),
        "a reference passage must never report itself surfaceable"
    );
    assert!(drafted.usage.is_some(), "usage must be reported");

    let serialized = serde_json::to_string(artifact).expect("artifact serializes");
    let load_error =
        load_law(&serialized).expect_err("an unreviewed candidate must be rejected by load_law");
    match load_error {
        monomyth_frameworks::LawError::MissingReviewer { law } => {
            assert_eq!(law, "mediating_plea");
        }
        other => panic!("expected MissingReviewer, got {other:?}"),
    }

    let mut reviewed: Value = serde_json::from_str(&serialized).expect("re-parse as JSON");
    reviewed["synthesis"]["reviewed_by"] = json!("a-human-reviewer");
    let reviewed_json = serde_json::to_string(&reviewed).expect("re-serialize");
    let loaded =
        load_law(&reviewed_json).expect("a reviewed artifact must load once reviewed_by is set");
    assert_eq!(loaded.law, "mediating_plea");
}

#[tokio::test]
async fn draft_law_refuses_when_reference_retrieval_is_empty() {
    let knowledge = test_knowledge();
    let llm = Llm::new(Box::new(CannedBackend::new(clean_candidate_json())));
    let request = base_request();

    let error = draft_law(&knowledge, &llm, &request)
        .await
        .expect_err("empty retrieval must refuse to synthesize ungrounded");

    match error {
        SynthesisError::NoReferenceGrounding { query } => {
            assert_eq!(query, request.query);
        }
        other => panic!("expected NoReferenceGrounding, got {other:?}"),
    }
}

#[tokio::test]
async fn draft_law_trips_the_anti_leak_gate_and_returns_no_artifact() {
    let knowledge = knowledge_with_reference_text().await;
    let llm = Llm::new(Box::new(CannedBackend::new(leaking_candidate_json())));
    let request = base_request();

    let error = draft_law(&knowledge, &llm, &request)
        .await
        .expect_err("a candidate embedding a verbatim source span must be refused");

    match error {
        SynthesisError::VerbatimOverlap {
            source_id,
            candidate_span,
        } => {
            assert_eq!(source_id, REFERENCE_SOURCE_ID);
            assert_eq!(
                candidate_span,
                "the suppliant implores a power in authority to"
            );
        }
        other => panic!("expected VerbatimOverlap, got {other:?}"),
    }
}

/// A sanity check that the fixed reference query used elsewhere in this file
/// actually retrieves the ingested passage (guards against the other tests
/// passing vacuously if retrieval silently returned nothing relevant).
#[tokio::test]
async fn reference_query_retrieves_the_ingested_synthetic_passage() {
    let knowledge = knowledge_with_reference_text().await;

    let passages = knowledge
        .retrieve(KnowledgeQuery::reference(
            "a plea to a powerful protector",
            5,
        ))
        .await
        .expect("retrieval should succeed");

    assert_eq!(passages.len(), 1);
    assert_eq!(passages[0].text, SYNTHETIC_REFERENCE_TEXT);
    assert_eq!(passages[0].source_id, REFERENCE_SOURCE_ID);
}
