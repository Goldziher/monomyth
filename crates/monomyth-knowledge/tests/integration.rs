//! End-to-end integration test over the real embedded backend.
//!
//! Ignored by default: it opens a file-backed `SqliteVectorStore` and a
//! `CoreEmbedder`, which downloads and runs a local ONNX model (network + disk).
//! Run explicitly with `cargo test -p monomyth-knowledge -- --ignored`.

use std::path::PathBuf;

use monomyth_knowledge::{IngestInput, Knowledge, KnowledgeQuery, Namespace};

fn temp_db_path() -> PathBuf {
    let mut path = std::env::temp_dir();
    let unique = format!("monomyth-knowledge-{}.db", std::process::id());
    path.push(unique);
    path
}

#[tokio::test]
#[ignore = "requires the local ONNX embedding model (download + disk)"]
async fn real_backend_ingest_then_surfaceable_retrieve() {
    let db_path = temp_db_path();
    let _ = std::fs::remove_file(&db_path);

    let knowledge = Knowledge::open(&db_path)
        .await
        .expect("open sqlite-backed knowledge layer");

    knowledge
        .ingest(
            "polti",
            IngestInput::new(
                "Situation one, Supplication: a Persecutor pursues a Suppliant who implores a Power in authority.",
            ),
        )
        .await
        .expect("ship source ingests");

    let passages = knowledge
        .retrieve(KnowledgeQuery::surfaceable(
            "someone begs a powerful protector for help",
            5,
        ))
        .await
        .expect("retrieval succeeds");

    assert!(!passages.is_empty(), "expected at least one passage");
    assert!(
        passages
            .iter()
            .all(|passage| passage.namespace == Namespace::Ship),
        "surfaceable retrieval must return ship-only passages"
    );
    assert!(passages.iter().any(|passage| passage.source_id == "polti"));

    let _ = std::fs::remove_file(&db_path);
}
