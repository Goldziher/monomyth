//! The document write path: resolving an upsert target, rewriting its fields and chunks.

use super::*;

/// The id to upsert `document` under: an existing document with the same
/// `external_id` (its chunks deleted, ready for rewriting), or a newly inserted row.
pub(super) fn resolve_document_id(
    conn: &Connection,
    collection: &str,
    document: &DocumentRecord,
    store_name: &str,
    vt: &str,
    ft: &str,
) -> RagResult<DocumentId> {
    let doc_id = match document.external_id.as_deref() {
        Some(ext) => {
            let mut stmt = conn
                .prepare_cached(
                    "SELECT id FROM documents WHERE collection = ?1 AND external_id = ?2",
                )
                .map_err(be)?;
            match stmt.query_row(params![collection, ext], |r| r.get::<_, String>(0)) {
                Ok(id) => {
                    delete_chunks_for_doc(conn, &id, vt, ft)?;
                    DocumentId(id)
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    insert_new_document(conn, collection, document, store_name)?
                }
                Err(e) => return Err(be(e)),
            }
        }
        None => insert_new_document(conn, collection, document, store_name)?,
    };
    Ok(doc_id)
}

/// Overwrite the stored fields of document `doc_id` with `document`'s.
pub(super) fn write_document_fields(
    conn: &Connection,
    doc_id: &DocumentId,
    document: &DocumentRecord,
) -> RagResult<()> {
    conn.execute(
        "UPDATE documents SET
             external_id = ?2, title = ?3, mime = ?4, source_uri = ?5,
             full_text = ?6, keywords = ?7, entities = ?8, labels = ?9,
             metadata = ?10, ingested_at = ?11
         WHERE id = ?1",
        params![
            doc_id.0,
            document.external_id,
            document.title,
            document.mime,
            document.source_uri,
            document.full_text,
            serde_json::to_string(&document.keywords).unwrap_or_default(),
            serde_json::to_string(&document.entities).unwrap_or_default(),
            serde_json::to_string(&document.labels).unwrap_or_default(),
            serde_json::to_string(&document.metadata).unwrap_or_default(),
            now_unix(),
        ],
    )
    .map_err(be)?;
    Ok(())
}

/// Insert one chunk row and its vector and full-text index entries.
pub(super) fn insert_chunk(
    conn: &Connection,
    doc_id: &DocumentId,
    collection: &str,
    chunk: &ChunkRecord,
    vt: &str,
    ft: &str,
) -> RagResult<()> {
    let chunk_id = ChunkId(format!("{}:{}", doc_id.0, chunk.ordinal));
    let blob = encode_vec(&chunk.embedding);
    let sparse_blob: Option<Vec<u8>> = chunk.sparse_embedding.as_ref().map(encode_sparse);
    let mv_blob: Option<Vec<u8>> = chunk.multi_vector.as_ref().map(encode_multi_vector);
    conn.execute(
        "INSERT INTO chunks
             (id, document_id, collection, ordinal, external_id, content,
              embedding, sparse, multi_vector, chunk_metadata)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            chunk_id.0,
            doc_id.0,
            collection,
            chunk.ordinal,
            chunk.external_id,
            chunk.content,
            blob,
            sparse_blob,
            mv_blob,
            serde_json::to_string(&chunk.chunk_metadata).unwrap_or_default(),
        ],
    )
    .map_err(be)?;

    conn.execute(
        &format!("INSERT INTO {vt} (chunk_id, embedding) VALUES (?1, ?2)"),
        params![chunk_id.0, blob],
    )
    .map_err(be)?;

    conn.execute(
        &format!("INSERT INTO {ft} (chunk_id, content) VALUES (?1, ?2)"),
        params![chunk_id.0, chunk.content],
    )
    .map_err(be)?;
    Ok(())
}

/// Insert a new document row with a rowid-derived id and return the id.
pub(super) fn insert_new_document(
    conn: &Connection,
    collection: &str,
    document: &DocumentRecord,
    store_name: &str,
) -> RagResult<DocumentId> {
    conn.execute(
        "INSERT INTO documents
             (id, collection, external_id, title, mime, source_uri,
              full_text, keywords, entities, labels, metadata, ingested_at)
         VALUES ('_tmp', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            collection,
            document.external_id,
            document.title,
            document.mime,
            document.source_uri,
            document.full_text,
            serde_json::to_string(&document.keywords).unwrap_or_default(),
            serde_json::to_string(&document.entities).unwrap_or_default(),
            serde_json::to_string(&document.labels).unwrap_or_default(),
            serde_json::to_string(&document.metadata).unwrap_or_default(),
            now_unix(),
        ],
    )
    .map_err(be)?;
    let rowid = conn.last_insert_rowid();
    let doc_id = DocumentId(format!("{store_name}-doc-{rowid}"));
    conn.execute(
        "UPDATE documents SET id = ?1 WHERE rowid = ?2",
        params![doc_id.0, rowid],
    )
    .map_err(be)?;
    Ok(doc_id)
}

/// Delete all chunks for `doc_id` from `chunks`, `vec0`, and `fts5` tables.
/// Returns the number of rows removed from `chunks`.
pub(super) fn delete_chunks_for_doc(
    conn: &Connection,
    doc_id: &str,
    vec_tbl: &str,
    fts_tbl: &str,
) -> RagResult<usize> {
    let chunk_ids: Vec<String> = {
        let mut stmt = conn
            .prepare_cached("SELECT id FROM chunks WHERE document_id = ?1")
            .map_err(be)?;
        stmt.query_map(params![doc_id], |r| r.get(0))
            .map_err(be)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(be)?
    };
    for cid in &chunk_ids {
        conn.execute(
            &format!("DELETE FROM {vec_tbl} WHERE chunk_id = ?1"),
            params![cid],
        )
        .map_err(be)?;
        conn.execute(
            &format!("DELETE FROM {fts_tbl} WHERE chunk_id = ?1"),
            params![cid],
        )
        .map_err(be)?;
    }
    let n = conn
        .execute("DELETE FROM chunks WHERE document_id = ?1", params![doc_id])
        .map_err(be)?;
    Ok(n)
}
