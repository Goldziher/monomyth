//! Offline integration tests for [`draft_law`], exercising the full
//! gather-grounding -> distill -> judge -> refine -> gate -> stamp pipeline
//! against an in-memory [`Knowledge`] and a canned, schema-dispatching
//! [`StructuredBackend`]. No network calls.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use monomyth_frameworks::load_law;
use monomyth_knowledge::rag::pipeline::Embedder;
use monomyth_knowledge::rag::{InMemoryVectorStore, RagResult};
use monomyth_knowledge::{EMBEDDING_DIM, IngestInput, Knowledge, KnowledgeQuery, Ledger};
use monomyth_llm::{BackendError, Llm, StructuredBackend, Usage};
use monomyth_synthesis::{DraftRequest, LoopConfig, SynthesisError, draft_law};
use serde_json::{Value, json};

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
    let store: Arc<dyn monomyth_knowledge::rag::VectorStore> =
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

/// A canned [`StructuredBackend`] that dispatches on `schema_name`: successive
/// calls for `"CandidateLaw"` pop from a queued list of candidate responses
/// (in order), and successive calls for `"JudgeVerdict"` pop from a queued
/// list of verdict responses (in order). This injects the transport seam at
/// the crate boundary (`StructuredBackend` is public) — it is not mocking an
/// internal service.
struct CannedBackend {
    candidate_responses: Mutex<Vec<Value>>,
    verdict_responses: Mutex<Vec<Value>>,
}

impl CannedBackend {
    /// A backend that always returns the same candidate response, with no
    /// judge responses queued (for tests that never reach a judge call).
    fn single_candidate(response: Value) -> Self {
        Self {
            candidate_responses: Mutex::new(vec![response]),
            verdict_responses: Mutex::new(Vec::new()),
        }
    }

    fn new(candidate_responses: Vec<Value>, verdict_responses: Vec<Value>) -> Self {
        Self {
            candidate_responses: Mutex::new(candidate_responses),
            verdict_responses: Mutex::new(verdict_responses),
        }
    }
}

#[async_trait]
impl StructuredBackend for CannedBackend {
    async fn complete_json(
        &self,
        _prompt: &str,
        schema_name: &str,
        _schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError> {
        let usage = Some(Usage {
            prompt_tokens: Some(100),
            completion_tokens: Some(50),
            total_tokens: Some(150),
        });

        let response = match schema_name {
            "CandidateLaw" => {
                let mut responses = self.candidate_responses.lock().expect("lock poisoned");
                if responses.is_empty() {
                    return Err(BackendError::new(
                        "no scripted CandidateLaw responses remain",
                    ));
                }
                responses.remove(0)
            }
            "JudgeVerdict" => {
                let mut responses = self.verdict_responses.lock().expect("lock poisoned");
                if responses.is_empty() {
                    return Err(BackendError::new(
                        "no scripted JudgeVerdict responses remain",
                    ));
                }
                responses.remove(0)
            }
            other => {
                return Err(BackendError::new(format!(
                    "unexpected schema name: {other}"
                )));
            }
        };

        Ok((response, usage))
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

/// A richer clean paraphrase adding a third item, used as the "refined"
/// candidate in the refine-then-accept test.
fn richer_candidate_json() -> Value {
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
            },
            {
                "name": "Resolution",
                "description": "The standoff concludes and balance is restored between the two poles."
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

/// A high judge verdict clearing the default initial passing bar (70.0).
fn high_verdict_json() -> Value {
    json!({
        "criteria": [
            { "name": "Exhaustiveness", "score": 90, "rationale": "covers the arc well" },
            { "name": "Source grounding", "score": 90, "rationale": "grounded" },
            { "name": "Ordering & non-overlap", "score": 90, "rationale": "ordered" },
            { "name": "Abstraction", "score": 90, "rationale": "abstract" },
            { "name": "Tier fit", "score": 90, "rationale": "macro-tier" }
        ],
        "missing_phases": [],
        "instructions": []
    })
}

/// A low judge verdict below the default initial passing bar (70.0), naming
/// a missing phase to drive a targeted retrieval and refinement.
fn low_verdict_json() -> Value {
    json!({
        "criteria": [
            { "name": "Exhaustiveness", "score": 40, "rationale": "missing the resolution phase" },
            { "name": "Source grounding", "score": 60, "rationale": "mostly grounded" },
            { "name": "Ordering & non-overlap", "score": 60, "rationale": "ordered so far" },
            { "name": "Abstraction", "score": 60, "rationale": "abstract enough" },
            { "name": "Tier fit", "score": 40, "rationale": "too coarse" }
        ],
        "missing_phases": ["a mediator between two poles"],
        "instructions": ["add a resolution phase"]
    })
}

fn base_request() -> DraftRequest {
    DraftRequest {
        law_id: "mediating_plea".to_owned(),
        domain: "myth".to_owned(),
        query: "a plea to a powerful protector".to_owned(),
        sub_queries: Vec::new(),
        // A synthetic provenance label: this offline test uses a CannedBackend,
        // so the model string is never routed — only stamped. Hardcoding a real
        // model id here would date the test and imply a routing that never happens.
        model: "test/stub-model".to_owned(),
        generated: "2026-07-11".to_owned(),
        loop_config: LoopConfig::default(),
    }
}

#[tokio::test]
async fn draft_law_happy_path_produces_a_pre_review_artifact_that_load_law_rejects_until_reviewed()
{
    let knowledge = knowledge_with_reference_text().await;
    let llm = Llm::new(Box::new(CannedBackend::new(
        vec![clean_candidate_json()],
        vec![high_verdict_json()],
    )));
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

    assert_eq!(
        drafted.iterations, 1,
        "a high first verdict must accept on the first judged iteration"
    );
    assert!(
        (drafted.final_score - 90.0).abs() < 1e-9,
        "final_score must equal the accepted verdict's weighted score, got {}",
        drafted.final_score
    );
    assert!(
        drafted.verdict.is_some(),
        "the best candidate's verdict must be recorded"
    );

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
    let llm = Llm::new(Box::new(CannedBackend::single_candidate(
        clean_candidate_json(),
    )));
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
async fn draft_law_refines_after_a_low_verdict_then_accepts_the_richer_candidate() {
    let knowledge = knowledge_with_reference_text().await;
    let llm = Llm::new(Box::new(CannedBackend::new(
        vec![clean_candidate_json(), richer_candidate_json()],
        vec![low_verdict_json(), high_verdict_json()],
    )));
    let request = base_request();

    let drafted = draft_law(&knowledge, &llm, &request)
        .await
        .expect("a refine-then-accept run must succeed");

    assert_eq!(
        drafted.iterations, 2,
        "must have judged twice: once low, once high"
    );
    assert_eq!(
        drafted.artifact.count, 3,
        "the accepted candidate must be the richer, three-item one"
    );
    assert_eq!(drafted.artifact.items[2].name, "Resolution");
    assert!(
        (drafted.final_score - 90.0).abs() < 1e-9,
        "final_score must reflect the accepted (high) verdict, got {}",
        drafted.final_score
    );
}

#[tokio::test]
async fn draft_law_keeps_the_best_candidate_when_no_iteration_ever_passes() {
    let knowledge = knowledge_with_reference_text().await;
    // Three low verdicts, none clearing even the initial (lowest) bar, so the
    // loop runs to `max_iterations` and returns the best (first) scored
    // candidate rather than looping forever.
    let llm = Llm::new(Box::new(CannedBackend::new(
        vec![
            clean_candidate_json(),
            richer_candidate_json(),
            richer_candidate_json(),
        ],
        vec![low_verdict_json(), low_verdict_json(), low_verdict_json()],
    )));
    let mut request = base_request();
    request.loop_config.max_iterations = 3;

    let drafted = draft_law(&knowledge, &llm, &request)
        .await
        .expect("a never-passing run must still return the best-scored candidate");

    assert_eq!(
        drafted.iterations, 3,
        "must exhaust max_iterations when no verdict ever clears its bar"
    );
    // Every low_verdict_json() scores identically, so the *first* judged
    // candidate remains best (later ones don't strictly exceed it).
    assert_eq!(
        drafted.artifact.count, 2,
        "must keep the first (initial) candidate, since no later score exceeded it"
    );
    let expected_low_score = {
        // (40*1.0 + 60*0.9 + 60*0.7 + 60*0.8 + 40*0.6) / (1.0+0.9+0.7+0.8+0.6)
        let numerator =
            40.0f64.mul_add(1.0, 60.0 * 0.9) + 60.0f64.mul_add(0.7, 60.0 * 0.8) + 40.0 * 0.6;
        let denominator = 1.0 + 0.9 + 0.7 + 0.8 + 0.6;
        numerator / denominator
    };
    assert!(
        (drafted.final_score - expected_low_score).abs() < 1e-9,
        "final_score must equal the low verdict's weighted score, got {} expected {}",
        drafted.final_score,
        expected_low_score
    );
}

#[tokio::test]
async fn draft_law_trips_the_anti_leak_gate_even_when_the_judge_scores_it_high() {
    let knowledge = knowledge_with_reference_text().await;
    let llm = Llm::new(Box::new(CannedBackend::new(
        vec![leaking_candidate_json()],
        vec![high_verdict_json()],
    )));
    let request = base_request();

    let error = draft_law(&knowledge, &llm, &request).await.expect_err(
        "a candidate embedding a verbatim source span must be refused regardless of judge score",
    );

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
