//! Multi-query coverage retrieval.
//!
//! A single retrieval query tends to cluster around one facet of a source
//! text. [`gather_grounding`] issues several queries — the seed query plus
//! caller-supplied coverage queries, or, mid-loop, the judge's named missing
//! phases — and unions the results, so the grounding available to the
//! distillation/refine prompts covers more of the source than any one query
//! would alone.

use std::collections::BTreeMap;

use monomyth_knowledge::{Knowledge, KnowledgeQuery, Passage};

use crate::error::SynthesisError;

/// Default `top_k` for each individual query issued by [`gather_grounding`].
///
/// Kept modest because coverage comes from the number of *distinct* queries
/// union'd, not from over-fetching on any single one.
pub(crate) const DEFAULT_PER_QUERY_TOP_K: u32 = 8;

/// Maximum number of passages [`gather_grounding`] returns after dedup, across
/// all queries combined. Bounds prompt size as the number of queries grows
/// (e.g. once the judge's `missing_phases` are folded in mid-loop).
pub(crate) const MAX_GROUNDING_PASSAGES: usize = 24;

/// Retrieve reference-namespace grounding for each of `queries`, union the
/// results, deduplicate, and return the highest-scoring `cap` passages.
///
/// Deduplication keys on `(source_id, normalized text)` — see
/// [`normalize_for_dedup`] — keeping the passage with the highest `score`
/// when a duplicate is found. The union is then sorted by score, descending,
/// and truncated to `cap`.
///
/// # Errors
///
/// Returns [`SynthesisError::Retrieval`] if any underlying query fails, and
/// [`SynthesisError::SurfaceablePassageInReferenceQuery`] (defense in depth)
/// if any retrieved passage reports itself surfaceable — a reference query
/// must never return ship material.
pub async fn gather_grounding(
    knowledge: &Knowledge,
    queries: &[String],
    per_query_top_k: u32,
    cap: usize,
) -> Result<Vec<Passage>, SynthesisError> {
    let mut deduped: BTreeMap<(String, String), Passage> = BTreeMap::new();

    for query in queries {
        let passages = knowledge
            .retrieve(KnowledgeQuery::reference(query.as_str(), per_query_top_k))
            .await?;

        for passage in passages {
            if passage.is_surfaceable() {
                return Err(SynthesisError::SurfaceablePassageInReferenceQuery {
                    source_id: passage.source_id,
                });
            }

            let key = (
                passage.source_id.clone(),
                normalize_for_dedup(&passage.text),
            );
            match deduped.get(&key) {
                Some(existing) if existing.score >= passage.score => {}
                _ => {
                    deduped.insert(key, passage);
                }
            }
        }
    }

    let mut merged: Vec<Passage> = deduped.into_values().collect();
    merged.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    merged.truncate(cap);

    Ok(merged)
}

/// Normalize `text` for dedup comparison: lowercase, with whitespace runs
/// collapsed to a single space and leading/trailing whitespace trimmed.
fn normalize_for_dedup(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use monomyth_knowledge::rag::pipeline::{
        Embedder, IngestRequest, RagPipelineConfig, ingest_document,
    };
    use monomyth_knowledge::rag::{CollectionSpec, InMemoryVectorStore, RagResult, VectorStore};
    use monomyth_knowledge::{EMBEDDING_DIM, IngestInput, Knowledge, Ledger, REFERENCE_COLLECTION};

    use super::gather_grounding;
    use crate::error::SynthesisError;

    const REFERENCE_SOURCE_ID: &str = "perseus";

    /// A deterministic, content-derived embedder: no ONNX, no network.
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
        let width = EMBEDDING_DIM as usize;
        let mut vector = vec![0.0f32; width];
        for (index, byte) in text.bytes().enumerate() {
            vector[index % width] += f32::from(byte) / 255.0;
        }
        vector
    }

    fn test_knowledge() -> Knowledge {
        let store: Arc<dyn monomyth_knowledge::rag::VectorStore> =
            Arc::new(InMemoryVectorStore::new("retrieval-test"));
        let embedder: Arc<dyn Embedder> = Arc::new(FakeEmbedder);
        let ledger = Ledger::load_embedded().expect("embedded manifest parses");
        Knowledge::with(store, embedder, ledger)
    }

    #[tokio::test]
    async fn dedups_a_passage_returned_by_two_overlapping_queries() {
        let knowledge = test_knowledge();
        knowledge
            .ingest_reference(
                REFERENCE_SOURCE_ID,
                IngestInput::new(
                    "The Suppliant implores a Power in authority to grant a boon of mercy, and the \
                     mediator stands between two poles until the trial is resolved.",
                ),
            )
            .await
            .expect("ingest succeeds");

        let queries = vec![
            "a plea to a powerful protector".to_owned(),
            "a mediator between two poles".to_owned(),
        ];

        let grounding = gather_grounding(&knowledge, &queries, 5, 24)
            .await
            .expect("gather_grounding succeeds");

        assert_eq!(
            grounding.len(),
            1,
            "the single ingested passage must be returned exactly once despite two queries \
             matching it, got {grounding:?}"
        );
        assert_eq!(grounding[0].source_id, REFERENCE_SOURCE_ID);
    }

    #[tokio::test]
    async fn returns_empty_grounding_for_an_empty_knowledge_base() {
        let knowledge = test_knowledge();

        let grounding = gather_grounding(&knowledge, &["anything".to_owned()], 5, 24)
            .await
            .expect("gather_grounding succeeds even with no matches");

        assert!(grounding.is_empty());
    }

    /// [`Knowledge::ingest`]/[`Knowledge::ingest_reference`] always stamp a
    /// document's namespace metadata consistent with the collection it lands
    /// in (ship source -> ship collection, reference source -> reference
    /// collection), so neither public method can construct a mistagged
    /// document. This test writes directly into the reference collection via
    /// `monomyth_knowledge::rag::pipeline::ingest_document`, on the same store `knowledge`
    /// reads from, bypassing the `Knowledge` ingest gate the way a genuinely
    /// incorrectly populated reference collection would — the same construction
    /// `monomyth-knowledge`'s own
    /// `surfaceable_retrieve_never_returns_a_reference_tagged_doc_from_the_ship_collection`
    /// test uses for the mirror-image scenario.
    ///
    /// `monomyth_knowledge::Knowledge::retrieve` itself already fails closed
    /// one layer earlier than [`gather_grounding`]'s own surfaceable check: a
    /// ship-tagged document pulled from the reference collection trips
    /// `build_passage`'s namespace-vs-collection invariant and never reaches
    /// this crate as a [`monomyth_knowledge::Passage`] at all, surfacing
    /// instead as `KnowledgeError::NamespaceViolation` ->
    /// [`SynthesisError::Retrieval`]. This test therefore asserts the
    /// actually reachable outcome: the mistagged document is still refused
    /// end to end, just one layer earlier than
    /// [`SynthesisError::SurfaceablePassageInReferenceQuery`] itself would
    /// catch it. [`gather_grounding`]'s own surfaceable check remains as
    /// belt-and-suspenders defense in depth should the upstream layer ever be
    /// weakened, even though this harness cannot currently drive a
    /// [`monomyth_knowledge::Passage`] past `Knowledge::retrieve` to exercise
    /// it directly (`gather_grounding` has no lower-level seam that accepts
    /// passages directly, only a live `Knowledge`).
    #[tokio::test]
    async fn refuses_a_mistagged_reference_document_one_layer_upstream_of_its_own_check() {
        let store: Arc<dyn VectorStore> =
            Arc::new(InMemoryVectorStore::new("retrieval-surfaceable-test"));
        let embedder: Arc<dyn Embedder> = Arc::new(FakeEmbedder);
        let ledger = Ledger::load_embedded().expect("embedded manifest parses");
        let knowledge = Knowledge::with(Arc::clone(&store), embedder, ledger);

        store
            .ensure_collection(&CollectionSpec::new(REFERENCE_COLLECTION, EMBEDDING_DIM))
            .await
            .expect("reference collection ensured");

        let metadata = serde_json::json!({
            "source_id": "smuggled",
            "namespace": "ship",
            "license": "CC0",
            "tier": "system",
            "domain": "myth",
        });
        let request = IngestRequest {
            full_text: "A shippable passage about a plea to a powerful protector.".to_owned(),
            metadata,
            ..IngestRequest::default()
        };
        let chunking = xberg::ChunkingConfig::default();
        let config = RagPipelineConfig {
            chunking: &chunking,
        };
        ingest_document(
            Arc::clone(&store),
            REFERENCE_COLLECTION,
            request,
            &config,
            &FakeEmbedder,
        )
        .await
        .expect("direct write into the reference collection (bypassing the gate) succeeds");

        let error = gather_grounding(
            &knowledge,
            &["a plea to a powerful protector".to_owned()],
            5,
            24,
        )
        .await
        .expect_err("a mistagged reference document must still be refused");

        match error {
            SynthesisError::Retrieval(_) => {}
            other => {
                panic!("expected Retrieval (namespace violation caught upstream), got {other:?}")
            }
        }
    }
}
