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
    let store: Arc<dyn crate::rag::VectorStore> = Arc::new(InMemoryVectorStore::new(STORE_NAME));
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
                end: start + u32::try_from("threshold".len()).expect("literal length fits in u32"),
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
    // This exact query — apostrophe, colon, comma — is a syntax error to ~keep
    // FTS5's MATCH parser if passed raw; the escaping + pre-embed path must ~keep
    // make it succeed rather than crash. ~keep
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
    let plain_json = serde_json::to_string(&plain_passages).expect("passages serialize to JSON");

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

    // A single salient term degenerates to a bare `ArrayContains`; several ~keep
    // combine under `Or`. Either way every leaf targets `doc.keywords`. ~keep
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

    // Vocabulary deliberately disjoint from the hero's-journey corpus, so no ~keep
    // stored keyword can match — the filtered fetch is empty. ~keep
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
    let plain_json = serde_json::to_string(&plain_passages).expect("passages serialize to JSON");

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
    // Apostrophe survives inside the quoted token; colon/comma no longer parse ~keep
    // as FTS5 operators. ~keep
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
