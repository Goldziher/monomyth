use super::*;
use crate::rag::filter::FilterField;
use crate::rag::query::RetrieveMode;

fn mk_chunk(ordinal: u32, content: &str, embedding: Vec<f32>) -> ChunkRecord {
    ChunkRecord {
        external_id: None,
        ordinal,
        content: content.to_string(),
        embedding,
        sparse_embedding: None,
        multi_vector: None,
        chunk_metadata: serde_json::Value::Null,
    }
}

fn sv(indices: &[u32], values: &[f32]) -> SparseVector {
    SparseVector {
        indices: indices.to_vec(),
        values: values.to_vec(),
    }
}

fn mv(num_tokens: u32, dim: u32, data: &[f32]) -> MultiVector {
    MultiVector {
        num_tokens,
        dim,
        data: data.to_vec(),
    }
}

fn mk_doc(full_text: &str) -> DocumentRecord {
    DocumentRecord {
        full_text: full_text.to_string(),
        ..Default::default()
    }
}

async fn empty_store() -> SqliteVectorStore {
    SqliteVectorStore::open_in_memory("test").await.unwrap()
}

async fn store_with_col(dim: u32) -> SqliteVectorStore {
    let store = empty_store().await;
    store
        .ensure_collection(&CollectionSpec::new("docs", dim))
        .await
        .unwrap();
    store
}

#[tokio::test]
async fn ensure_collection_creates_and_is_idempotent() {
    let store = empty_store().await;
    let spec = CollectionSpec::new("col", 4);
    store.ensure_collection(&spec).await.unwrap();
    store.ensure_collection(&spec).await.unwrap();
    let got = store.get_collection("col").await.unwrap().unwrap();
    assert_eq!(got.embedding_dim, 4);
}

#[tokio::test]
async fn ensure_collection_rejects_dim_mismatch() {
    let store = empty_store().await;
    store
        .ensure_collection(&CollectionSpec::new("col", 4))
        .await
        .unwrap();
    let err = store
        .ensure_collection(&CollectionSpec::new("col", 8))
        .await
        .unwrap_err();
    assert!(matches!(err, RagError::CollectionAlreadyExists(_)));
}

#[tokio::test]
async fn upsert_then_retrieve_vector_roundtrip() {
    let store = store_with_col(3).await;
    let doc = mk_doc("hello world");
    let chunks = vec![
        mk_chunk(0, "near", vec![1.0, 0.0, 0.0]),
        mk_chunk(1, "far", vec![0.0, 1.0, 0.0]),
    ];
    let doc_id = store.upsert_document("docs", &doc, &chunks).await.unwrap();
    assert!(doc_id.0.starts_with("test-doc-"));

    let q = RetrieveQuery {
        query_vector: Some(vec![1.0, 0.0, 0.0]),
        include_content: true,
        ..RetrieveQuery::vector(10)
    };
    let out = store.retrieve("docs", &q).await.unwrap();
    assert_eq!(out.mode, RetrieveMode::Vector);
    assert_eq!(out.chunks.len(), 2);
    assert_eq!(out.chunks[0].content.as_deref(), Some("near"));
}

#[tokio::test]
async fn dimension_mismatch_rejected_at_upsert() {
    let store = store_with_col(3).await;
    let bad = vec![mk_chunk(0, "x", vec![1.0, 0.0])];
    let err = store
        .upsert_document("docs", &mk_doc(""), &bad)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        RagError::EmbeddingDimMismatch {
            expected: 3,
            got: 2
        }
    ));
}

#[tokio::test]
async fn external_id_upsert_replaces_chunks() {
    let store = store_with_col(2).await;
    let doc = DocumentRecord {
        external_id: Some("ext-1".to_string()),
        ..Default::default()
    };
    store
        .upsert_document("docs", &doc, &[mk_chunk(0, "v1", vec![1.0, 0.0])])
        .await
        .unwrap();
    store
        .upsert_document("docs", &doc, &[mk_chunk(0, "v2", vec![0.0, 1.0])])
        .await
        .unwrap();
    let stats = store.collection_stats("docs").await.unwrap();
    assert_eq!(stats.documents, 1, "should still be one document");
    assert_eq!(stats.chunks, 1, "old chunk replaced by new chunk");

    let q = RetrieveQuery {
        query_vector: Some(vec![0.0, 1.0]),
        include_content: true,
        ..RetrieveQuery::vector(10)
    };
    let out = store.retrieve("docs", &q).await.unwrap();
    assert_eq!(out.chunks[0].content.as_deref(), Some("v2"));
}

#[tokio::test]
async fn full_text_retrieve_roundtrip() {
    let store = store_with_col(2).await;
    let doc_a = mk_doc("document about rust programming");
    let doc_b = mk_doc("document about python scripting");
    store
        .upsert_document(
            "docs",
            &doc_a,
            &[mk_chunk(0, "Rust is fast and safe", vec![1.0, 0.0])],
        )
        .await
        .unwrap();
    store
        .upsert_document(
            "docs",
            &doc_b,
            &[mk_chunk(0, "Python is easy to learn", vec![0.0, 1.0])],
        )
        .await
        .unwrap();

    let q = RetrieveQuery {
        mode: RetrieveMode::FullText,
        query_text: Some("Rust".to_string()),
        include_content: true,
        ..RetrieveQuery::vector(5)
    };
    let out = store.retrieve("docs", &q).await.unwrap();
    assert!(!out.chunks.is_empty());
    assert!(
        out.chunks[0]
            .content
            .as_deref()
            .unwrap_or("")
            .contains("Rust"),
        "top result should mention Rust"
    );
}

#[tokio::test]
async fn hybrid_retrieve_returns_results() {
    let store = store_with_col(2).await;
    let doc = mk_doc("hybrid search document");
    store
        .upsert_document(
            "docs",
            &doc,
            &[mk_chunk(0, "hybrid search content", vec![1.0, 0.0])],
        )
        .await
        .unwrap();

    let q = RetrieveQuery {
        mode: RetrieveMode::Hybrid,
        query_text: Some("hybrid".to_string()),
        query_vector: Some(vec![1.0, 0.0]),
        include_content: true,
        ..RetrieveQuery::vector(5)
    };
    let out = store.retrieve("docs", &q).await.unwrap();
    assert!(!out.chunks.is_empty());
    let ps = out.chunks[0].primary_score;
    assert!(matches!(ps, PrimaryScore::Hybrid { .. }));
}

#[test]
fn sparse_blob_roundtrips() {
    let original = sv(&[1, 7, 900], &[0.5, 1.5, 2.0]);
    let decoded = decode_sparse(&encode_sparse(&original)).unwrap();
    assert_eq!(decoded, original);
    assert_eq!(
        decode_sparse(&encode_sparse(&SparseVector::default())),
        Some(SparseVector::default())
    );
}

#[test]
fn multi_vector_blob_roundtrips() {
    let original = mv(2, 3, &[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]);
    let decoded = decode_multi_vector(&encode_multi_vector(&original)).unwrap();
    assert_eq!(decoded, original);
}

#[test]
fn decode_sparse_rejects_truncated_blob() {
    let mut bytes = encode_sparse(&sv(&[1, 2], &[0.5, 0.5]));
    bytes.truncate(bytes.len() - 1);
    assert_eq!(decode_sparse(&bytes), None);
    assert_eq!(decode_sparse(&[]), None);
}

#[test]
fn decode_multi_vector_rejects_truncated_blob() {
    let mut bytes = encode_multi_vector(&mv(2, 2, &[1.0, 2.0, 3.0, 4.0]));
    bytes.truncate(bytes.len() - 1);
    assert_eq!(decode_multi_vector(&bytes), None);
    assert_eq!(decode_multi_vector(&[0, 0, 0]), None);
}

async fn upsert_sparse_chunk(
    store: &SqliteVectorStore,
    content: &str,
    embedding: Vec<f32>,
    sparse: SparseVector,
) {
    let mut chunk = mk_chunk(0, content, embedding);
    chunk.sparse_embedding = Some(sparse);
    store
        .upsert_document("docs", &mk_doc(content), &[chunk])
        .await
        .unwrap();
}

#[tokio::test]
async fn sparse_retrieve_ranks_by_overlap() {
    let store = store_with_col(2).await;
    upsert_sparse_chunk(&store, "match", vec![1.0, 0.0], sv(&[1, 3], &[2.0, 2.0])).await;
    upsert_sparse_chunk(&store, "other", vec![0.0, 1.0], sv(&[5, 9], &[2.0, 2.0])).await;

    let q = RetrieveQuery {
        mode: RetrieveMode::Sparse,
        query_sparse: Some(sv(&[1, 3], &[1.0, 1.0])),
        include_content: true,
        ..RetrieveQuery::vector(10)
    };
    let out = store.retrieve("docs", &q).await.unwrap();
    assert_eq!(out.mode, RetrieveMode::Sparse);
    assert_eq!(out.chunks.len(), 1, "only the overlapping chunk scores > 0");
    assert_eq!(out.chunks[0].content.as_deref(), Some("match"));
    assert!(matches!(out.chunks[0].primary_score, PrimaryScore::Sparse(s) if s > 0.0));
}

#[tokio::test]
async fn sparse_retrieve_rejects_malformed_query() {
    let store = store_with_col(2).await;
    let q = RetrieveQuery {
        mode: RetrieveMode::Sparse,
        query_sparse: Some(sv(&[3, 1], &[1.0, 1.0])),
        ..RetrieveQuery::vector(10)
    };
    let err = store.retrieve("docs", &q).await.unwrap_err();
    assert!(matches!(err, RagError::InvalidQuery(_)));
}

#[tokio::test]
async fn upsert_rejects_malformed_sparse_chunk() {
    let store = store_with_col(2).await;
    let mut chunk = mk_chunk(0, "bad", vec![1.0, 0.0]);
    chunk.sparse_embedding = Some(sv(&[1, 2, 3], &[1.0]));
    let err = store
        .upsert_document("docs", &mk_doc("bad"), &[chunk])
        .await
        .unwrap_err();
    assert!(matches!(err, RagError::InvalidQuery(_)));
}

#[tokio::test]
async fn late_interaction_rescores_dense_candidates() {
    let store = store_with_col(2).await;
    let mut a = mk_chunk(0, "a", vec![1.0, 0.0]);
    a.multi_vector = Some(mv(1, 2, &[1.0, 0.0]));
    let mut b = mk_chunk(0, "b", vec![0.9, 0.1]);
    b.multi_vector = Some(mv(1, 2, &[0.0, 1.0]));
    store
        .upsert_document("docs", &mk_doc("a"), &[a])
        .await
        .unwrap();
    store
        .upsert_document("docs", &mk_doc("b"), &[b])
        .await
        .unwrap();

    let q = RetrieveQuery {
        mode: RetrieveMode::LateInteraction,
        query_vector: Some(vec![1.0, 0.0]),
        query_multi_vector: Some(mv(1, 2, &[1.0, 0.0])),
        include_content: true,
        ..RetrieveQuery::vector(10)
    };
    let out = store.retrieve("docs", &q).await.unwrap();
    assert_eq!(out.mode, RetrieveMode::LateInteraction);
    assert_eq!(
        out.chunks[0].content.as_deref(),
        Some("a"),
        "MaxSim favours the aligned token"
    );
    assert!(matches!(
        out.chunks[0].primary_score,
        PrimaryScore::LateInteraction(_)
    ));
}

#[tokio::test]
async fn late_interaction_without_query_vector_returns_clear_error() {
    let store = store_with_col(2).await;
    let q = RetrieveQuery {
        mode: RetrieveMode::LateInteraction,
        query_text: Some("hi".to_string()),
        query_multi_vector: Some(mv(1, 2, &[1.0, 0.0])),
        ..RetrieveQuery::vector(10)
    };
    let err = store.retrieve("docs", &q).await.unwrap_err();
    let RagError::InvalidQuery(msg) = err else {
        panic!("expected InvalidQuery, got {err:?}");
    };
    assert!(
        msg.contains("query_vector") && msg.contains("sqlite"),
        "error should explain that the sqlite backend requires query_vector: {msg}"
    );
}

#[tokio::test]
async fn hybrid_fuses_sparse_arm() {
    let store = store_with_col(2).await;
    upsert_sparse_chunk(
        &store,
        "hybrid sparse content",
        vec![1.0, 0.0],
        sv(&[1, 2], &[1.0, 1.0]),
    )
    .await;

    let q = RetrieveQuery {
        mode: RetrieveMode::Hybrid,
        query_text: Some("hybrid".to_string()),
        query_vector: Some(vec![1.0, 0.0]),
        query_sparse: Some(sv(&[1, 2], &[1.0, 1.0])),
        include_content: true,
        ..RetrieveQuery::vector(5)
    };
    let out = store.retrieve("docs", &q).await.unwrap();
    assert!(!out.chunks.is_empty());
    let PrimaryScore::Hybrid { sparse, .. } = out.chunks[0].primary_score else {
        panic!("expected hybrid primary score");
    };
    assert!(
        sparse > 0.0,
        "sparse arm should contribute to the fused score"
    );
}

#[tokio::test]
async fn delete_by_filter_removes_matching_documents() {
    let store = store_with_col(2).await;
    let keep = DocumentRecord {
        title: Some("keep".to_string()),
        ..Default::default()
    };
    let drop = DocumentRecord {
        title: Some("drop".to_string()),
        ..Default::default()
    };
    store
        .upsert_document("docs", &keep, &[mk_chunk(0, "keep", vec![1.0, 0.0])])
        .await
        .unwrap();
    store
        .upsert_document("docs", &drop, &[mk_chunk(0, "drop", vec![0.0, 1.0])])
        .await
        .unwrap();

    let filter = Filter::Eq {
        field: FilterField("doc.title".to_string()),
        value: serde_json::json!("drop"),
    };
    let removed = store.delete_by_filter("docs", &filter).await.unwrap();
    assert_eq!(removed, 1);

    let stats = store.collection_stats("docs").await.unwrap();
    assert_eq!(stats.documents, 1);
    assert_eq!(stats.chunks, 1);
}

#[tokio::test]
async fn drop_collection_removes_all_data() {
    let store = store_with_col(2).await;
    store
        .upsert_document("docs", &mk_doc("x"), &[mk_chunk(0, "x", vec![1.0, 0.0])])
        .await
        .unwrap();
    store.drop_collection("docs").await.unwrap();
    let spec = store.get_collection("docs").await.unwrap();
    assert!(spec.is_none());
}

#[tokio::test]
async fn drop_nonexistent_collection_errors() {
    let store = empty_store().await;
    let err = store.drop_collection("ghost").await.unwrap_err();
    assert!(matches!(err, RagError::CollectionNotFound(_)));
}

#[tokio::test]
async fn collection_stats_tracks_documents_and_chunks() {
    let store = store_with_col(2).await;
    store
        .upsert_document(
            "docs",
            &mk_doc("a"),
            &[
                mk_chunk(0, "c0", vec![1.0, 0.0]),
                mk_chunk(1, "c1", vec![0.0, 1.0]),
            ],
        )
        .await
        .unwrap();
    let stats = store.collection_stats("docs").await.unwrap();
    assert_eq!(stats.documents, 1);
    assert_eq!(stats.chunks, 2);
    assert!(stats.last_ingested_at.is_some());
}
