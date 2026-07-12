//! CI/audit-time verification that stored document metadata agrees with the
//! license ledger — the **third** enforcement point of ADR-0005's licensing
//! invariant (after the ingest gate and the retrieval filter/[`build_passage`]
//! check).
//!
//! [`Knowledge::audit_stored_metadata`] re-derives, for every document actually
//! sitting in each collection, what the ledger says its `namespace` / `tier` /
//! `license` / `domain` ought to be (looked up by the stored `source_id`) and
//! compares it against what is actually stored. It also asserts collection
//! containment: nothing but `ship`-namespace documents may sit in the `ship`
//! collection, and nothing but `reference`-namespace documents may sit in the
//! `reference` collection.

use std::sync::Arc;

use serde_json::Value;

use crate::error::KnowledgeError;
use crate::ledger::{Ledger, Namespace, REFERENCE_COLLECTION, SHIP_COLLECTION};
use crate::rag::pipeline::retrieve as pipeline_retrieve;
use crate::rag::{RetrieveQuery, RetrievedChunk};
use crate::{META_DOMAIN, META_LICENSE, META_NAMESPACE, META_SOURCE_ID, META_TIER};

/// Neutral query text used to enumerate stored documents.
///
/// The xberg [`crate::rag::VectorStore`] API has no full-scan / list-all
/// operation — every retrieval is a similarity query. We approximate a full
/// scan with a broad, topic-neutral vector query at the maximum allowed
/// `top_k` ([`crate::rag::MAX_TOP_K`]) and no filter, which returns every
/// stored chunk ranked by similarity to the neutral text rather than a true
/// enumeration. This is a real limitation: a collection holding more distinct
/// documents than fit in one `top_k`-sized pull, or documents whose vectors
/// are unusually distant from this neutral point in embedding space, could be
/// under-sampled. It is adequate for a CI-scale fixture corpus; a true
/// enumeration would need a `VectorStore` method xberg does not expose today.
const NEUTRAL_QUERY_TEXT: &str = "the corpus of stored documents";

impl crate::Knowledge {
    /// Audit every document stored in the `ship` and `reference` collections
    /// against the license ledger.
    ///
    /// For each collection, enumerates stored documents via a broad retrieve
    /// (see [`NEUTRAL_QUERY_TEXT`]) and, for every distinct document found,
    /// checks that its stored `namespace` / `tier` / `license` / `domain`
    /// metadata equals its ledger entry (looked up by the stored
    /// `source_id`), and that the collection it was found in matches its
    /// namespace (ship documents only in the ship collection, reference
    /// documents only in the reference collection).
    ///
    /// # Errors
    ///
    /// [`KnowledgeError::AuditViolation`] on the first metadata field that
    /// disagrees with the ledger, or on a document misfiled outside its
    /// namespace's collection. [`KnowledgeError::UndeclaredSource`] if a
    /// stored document's `source_id` is not declared in the ledger.
    /// [`KnowledgeError::MissingMetadata`] / [`KnowledgeError::Store`]
    /// propagate from the underlying retrieve.
    pub async fn audit_stored_metadata(&self) -> Result<(), KnowledgeError> {
        for collection in [SHIP_COLLECTION, REFERENCE_COLLECTION] {
            self.ensure_collection(collection).await?;
            let chunks =
                broad_retrieve(Arc::clone(&self.store), collection, self.embedder.as_ref()).await?;
            audit_collection(collection, &chunks, &self.ledger)?;
        }
        Ok(())
    }
}

/// Pull every chunk visible to a broad, unfiltered query against `collection`.
///
/// See [`NEUTRAL_QUERY_TEXT`] for why this is a broad retrieve rather than a
/// true enumeration.
async fn broad_retrieve(
    store: Arc<dyn crate::rag::VectorStore>,
    collection: &str,
    embedder: &dyn crate::rag::pipeline::Embedder,
) -> Result<Vec<RetrievedChunk>, KnowledgeError> {
    let query = RetrieveQuery {
        query_text: Some(NEUTRAL_QUERY_TEXT.to_owned()),
        include_content: false,
        include_document: true,
        ..RetrieveQuery::vector(crate::rag::MAX_TOP_K)
    };
    pipeline_retrieve(store, collection, query, Some(embedder))
        .await
        .map_err(|error| KnowledgeError::store("auditing stored documents", error))
}

/// Check every distinct document among `chunks` (deduped by `document_id`)
/// against `ledger`, in the context of having been retrieved from
/// `collection`.
fn audit_collection(
    collection: &str,
    chunks: &[RetrievedChunk],
    ledger: &Ledger,
) -> Result<(), KnowledgeError> {
    let mut seen = std::collections::BTreeSet::new();
    for chunk in chunks {
        if !seen.insert(chunk.document_id.0.clone()) {
            continue;
        }
        let metadata = chunk
            .document
            .as_ref()
            .map(|doc| &doc.metadata)
            .ok_or_else(|| KnowledgeError::MissingMetadata {
                collection: collection.to_owned(),
                field: "document",
            })?;
        audit_document(collection, metadata, ledger)?;
    }
    Ok(())
}

/// Check one stored document's metadata against its ledger entry.
fn audit_document(
    collection: &str,
    metadata: &Value,
    ledger: &Ledger,
) -> Result<(), KnowledgeError> {
    let source_id = metadata
        .get(META_SOURCE_ID)
        .and_then(Value::as_str)
        .ok_or_else(|| KnowledgeError::MissingMetadata {
            collection: collection.to_owned(),
            field: META_SOURCE_ID,
        })?
        .to_owned();

    let entry = ledger
        .get(&source_id)
        .ok_or_else(|| KnowledgeError::UndeclaredSource {
            id: source_id.clone(),
        })?;

    let expected_namespace = entry.namespace.as_wire();
    check_field(
        collection,
        &source_id,
        META_NAMESPACE,
        expected_namespace,
        metadata,
    )?;
    check_field(
        collection,
        &source_id,
        META_TIER,
        entry.tier.as_wire(),
        metadata,
    )?;
    check_field(
        collection,
        &source_id,
        META_LICENSE,
        &entry.license,
        metadata,
    )?;
    check_field(collection, &source_id, META_DOMAIN, &entry.domain, metadata)?;

    let expected_collection = match entry.namespace {
        Namespace::Ship => SHIP_COLLECTION,
        Namespace::Reference => REFERENCE_COLLECTION,
    };
    if expected_collection != collection {
        return Err(KnowledgeError::AuditViolation {
            collection: collection.to_owned(),
            source_id,
            field: "collection",
            expected: expected_collection.to_owned(),
            found: collection.to_owned(),
        });
    }

    Ok(())
}

/// Compare one stored metadata field against its ledger-expected value.
fn check_field(
    collection: &str,
    source_id: &str,
    field: &'static str,
    expected: &str,
    metadata: &Value,
) -> Result<(), KnowledgeError> {
    let found = metadata.get(field).and_then(Value::as_str).ok_or_else(|| {
        KnowledgeError::MissingMetadata {
            collection: collection.to_owned(),
            field,
        }
    })?;
    if found != expected {
        return Err(KnowledgeError::AuditViolation {
            collection: collection.to_owned(),
            source_id: source_id.to_owned(),
            field,
            expected: expected.to_owned(),
            found: found.to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::rag::pipeline::{IngestRequest, RagPipelineConfig, ingest_document};
    use crate::rag::{CollectionSpec, InMemoryVectorStore};

    use super::*;
    use crate::{EMBEDDING_DIM, Ledger};

    /// A deterministic fake embedder, mirroring the one in `crate::tests`
    /// (private to that module, so duplicated here rather than shared).
    #[derive(Debug)]
    struct AuditFakeEmbedder;

    #[async_trait::async_trait]
    impl crate::rag::pipeline::Embedder for AuditFakeEmbedder {
        async fn embed(&self, texts: Vec<String>) -> crate::rag::RagResult<Vec<Vec<f32>>> {
            Ok(texts
                .iter()
                .map(|_| vec![0.1f32; EMBEDDING_DIM as usize])
                .collect())
        }
    }

    /// A real declared ship source (public domain, `framework` domain), reused
    /// from the embedded manifest so the audit tests exercise the actual
    /// ledger rather than a synthetic one.
    const SHIP_SOURCE_ID: &str = "polti";
    /// A real declared reference source (`NonCommercial`, `myth` domain).
    const REFERENCE_SOURCE_ID: &str = "perseus";

    /// The embedded corpus ledger — `Ledger` has no `Deserialize` impl of its
    /// own (only `load_embedded` builds one), so tests exercise real declared
    /// sources rather than constructing a synthetic ledger.
    fn test_ledger() -> Ledger {
        Ledger::load_embedded().expect("embedded manifest parses")
    }

    fn store() -> Arc<dyn crate::rag::VectorStore> {
        Arc::new(InMemoryVectorStore::new("audit-test"))
    }

    async fn ensure(store: &Arc<dyn crate::rag::VectorStore>, collection: &str) {
        store
            .ensure_collection(&CollectionSpec::new(collection, EMBEDDING_DIM))
            .await
            .expect("collection ensured");
    }

    /// Write a document directly through the pipeline (bypassing
    /// `Knowledge::ingest`'s ship gate), with metadata under full test
    /// control — the lowest-level path available to construct a tampered
    /// fixture without adding a test-only mutator to production code.
    async fn write_document(
        store: &Arc<dyn crate::rag::VectorStore>,
        collection: &str,
        metadata: Value,
    ) {
        let request = IngestRequest {
            full_text: "Some stored passage text for the audit fixture.".to_owned(),
            metadata,
            ..IngestRequest::default()
        };
        let chunking = xberg::ChunkingConfig::default();
        let config = RagPipelineConfig {
            chunking: &chunking,
        };

        ingest_document(
            Arc::clone(store),
            collection,
            request,
            &config,
            &AuditFakeEmbedder,
        )
        .await
        .expect("fixture document ingests");
    }

    /// Metadata for `SHIP_SOURCE_ID` (`polti`) exactly as its ledger entry
    /// declares it — a correctly-tagged fixture.
    fn ship_metadata(source_id: &str) -> Value {
        serde_json::json!({
            META_SOURCE_ID: source_id,
            META_NAMESPACE: "ship",
            META_LICENSE: "PD (1916 tr.)",
            META_TIER: "public_domain",
            META_DOMAIN: "framework",
        })
    }

    /// Metadata for `REFERENCE_SOURCE_ID` (`perseus`) exactly as its ledger
    /// entry declares it — a correctly-tagged fixture.
    fn reference_metadata(source_id: &str) -> Value {
        serde_json::json!({
            META_SOURCE_ID: source_id,
            META_NAMESPACE: "reference",
            META_LICENSE: "CC-BY-NC-SA-3.0",
            META_TIER: "noncommercial",
            META_DOMAIN: "myth",
        })
    }

    #[tokio::test]
    async fn should_pass_when_every_stored_document_agrees_with_its_ledger_entry() {
        let store = store();
        ensure(&store, SHIP_COLLECTION).await;
        ensure(&store, REFERENCE_COLLECTION).await;
        write_document(&store, SHIP_COLLECTION, ship_metadata(SHIP_SOURCE_ID)).await;
        write_document(
            &store,
            REFERENCE_COLLECTION,
            reference_metadata(REFERENCE_SOURCE_ID),
        )
        .await;

        let knowledge = crate::Knowledge::with(
            Arc::clone(&store),
            Arc::new(AuditFakeEmbedder),
            test_ledger(),
        );

        knowledge
            .audit_stored_metadata()
            .await
            .expect("a clean ship+reference corpus must pass audit");
    }

    #[tokio::test]
    async fn should_return_audit_violation_when_stored_namespace_disagrees_with_ledger() {
        let store = store();
        ensure(&store, SHIP_COLLECTION).await;
        ensure(&store, REFERENCE_COLLECTION).await;

        let mut tampered = ship_metadata(SHIP_SOURCE_ID);
        tampered[META_NAMESPACE] = Value::String("reference".to_owned());
        write_document(&store, SHIP_COLLECTION, tampered).await;

        let knowledge = crate::Knowledge::with(
            Arc::clone(&store),
            Arc::new(AuditFakeEmbedder),
            test_ledger(),
        );

        let error = knowledge
            .audit_stored_metadata()
            .await
            .expect_err("a tampered namespace must be caught by the audit");
        match error {
            KnowledgeError::AuditViolation {
                collection,
                source_id,
                field,
                expected,
                found,
            } => {
                assert_eq!(collection, SHIP_COLLECTION);
                assert_eq!(source_id, SHIP_SOURCE_ID);
                assert_eq!(field, META_NAMESPACE);
                assert_eq!(expected, "ship");
                assert_eq!(found, "reference");
            }
            other => panic!("expected AuditViolation, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_return_audit_violation_when_stored_tier_disagrees_with_ledger() {
        let store = store();
        ensure(&store, SHIP_COLLECTION).await;
        ensure(&store, REFERENCE_COLLECTION).await;

        let mut tampered = ship_metadata(SHIP_SOURCE_ID);
        tampered[META_TIER] = Value::String("cc0".to_owned());
        write_document(&store, SHIP_COLLECTION, tampered).await;

        let knowledge = crate::Knowledge::with(
            Arc::clone(&store),
            Arc::new(AuditFakeEmbedder),
            test_ledger(),
        );

        let error = knowledge
            .audit_stored_metadata()
            .await
            .expect_err("a tampered tier must be caught by the audit");
        match error {
            KnowledgeError::AuditViolation {
                field,
                expected,
                found,
                ..
            } => {
                assert_eq!(field, META_TIER);
                assert_eq!(expected, "public_domain");
                assert_eq!(found, "cc0");
            }
            other => panic!("expected AuditViolation, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_return_audit_violation_when_stored_license_disagrees_with_ledger() {
        let store = store();
        ensure(&store, SHIP_COLLECTION).await;
        ensure(&store, REFERENCE_COLLECTION).await;

        let mut tampered = ship_metadata(SHIP_SOURCE_ID);
        tampered[META_LICENSE] = Value::String("CC-BY-4.0".to_owned());
        write_document(&store, SHIP_COLLECTION, tampered).await;

        let knowledge = crate::Knowledge::with(
            Arc::clone(&store),
            Arc::new(AuditFakeEmbedder),
            test_ledger(),
        );

        let error = knowledge
            .audit_stored_metadata()
            .await
            .expect_err("a tampered license must be caught by the audit");
        match error {
            KnowledgeError::AuditViolation {
                field,
                expected,
                found,
                ..
            } => {
                assert_eq!(field, META_LICENSE);
                assert_eq!(expected, "PD (1916 tr.)");
                assert_eq!(found, "CC-BY-4.0");
            }
            other => panic!("expected AuditViolation, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_return_undeclared_source_when_stored_source_id_is_not_in_the_ledger() {
        let store = store();
        ensure(&store, SHIP_COLLECTION).await;
        ensure(&store, REFERENCE_COLLECTION).await;
        write_document(&store, SHIP_COLLECTION, ship_metadata("not_in_ledger")).await;

        let knowledge = crate::Knowledge::with(
            Arc::clone(&store),
            Arc::new(AuditFakeEmbedder),
            test_ledger(),
        );

        let error = knowledge
            .audit_stored_metadata()
            .await
            .expect_err("an undeclared source id must be rejected");
        match error {
            KnowledgeError::UndeclaredSource { id } => assert_eq!(id, "not_in_ledger"),
            other => panic!("expected UndeclaredSource, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_return_audit_violation_when_a_ship_document_is_stored_in_the_reference_collection()
     {
        let store = store();
        ensure(&store, SHIP_COLLECTION).await;
        ensure(&store, REFERENCE_COLLECTION).await;
        write_document(&store, REFERENCE_COLLECTION, ship_metadata(SHIP_SOURCE_ID)).await;

        let knowledge = crate::Knowledge::with(
            Arc::clone(&store),
            Arc::new(AuditFakeEmbedder),
            test_ledger(),
        );

        let error = knowledge.audit_stored_metadata().await.expect_err(
            "a ship-namespace document stored in the reference collection must fail audit",
        );
        match error {
            KnowledgeError::AuditViolation {
                collection,
                field,
                expected,
                found,
                ..
            } => {
                assert_eq!(collection, REFERENCE_COLLECTION);
                assert_eq!(field, "collection");
                assert_eq!(expected, SHIP_COLLECTION);
                assert_eq!(found, REFERENCE_COLLECTION);
            }
            other => panic!("expected AuditViolation, got {other:?}"),
        }
    }
}
