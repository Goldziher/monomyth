//! The retrieval modes: vector, full-text, hybrid, sparse and late interaction.

use super::*;

/// Apply a filter to a list of `RetrievedChunk`s, loading the document and
/// chunk records from the database as needed for evaluation.
pub(super) fn filter_chunks(
    conn: &Connection,
    chunks: Vec<RetrievedChunk>,
    filter: &Filter,
) -> RagResult<Vec<RetrievedChunk>> {
    let mut out = Vec::with_capacity(chunks.len());
    for chunk in chunks {
        let Some((doc_rec, _)) = load_document_record(conn, &chunk.document_id.0).map_err(be)?
        else {
            continue;
        };
        let Some(chunk_rec) = conn
            .query_row(
                "SELECT ordinal, external_id, content, embedding, chunk_metadata
                 FROM chunks WHERE id = ?1",
                params![chunk.id.0],
                |r| {
                    let meta_str: String = r.get(4)?;
                    Ok(ChunkRecord {
                        ordinal: r.get::<_, u32>(0)?,
                        external_id: r.get(1)?,
                        content: r.get(2)?,
                        embedding: Vec::new(),
                        sparse_embedding: None,
                        multi_vector: None,
                        chunk_metadata: serde_json::from_str(&meta_str)
                            .unwrap_or(serde_json::Value::Null),
                    })
                },
            )
            .ok()
        else {
            continue;
        };
        if eval_filter(filter, &doc_rec, &chunk_rec) {
            out.push(chunk);
        }
    }
    Ok(out)
}

pub(super) fn retrieve_vector(
    conn: &Connection,
    vec_tbl: &str,
    query_vector: &[f32],
    candidate_k: i64,
    query: &RetrieveQuery,
) -> RagResult<Vec<RetrievedChunk>> {
    let blob = encode_vec(query_vector);
    let sql = format!(
        "SELECT chunk_id, distance FROM {vec_tbl}
         WHERE embedding MATCH ?1 AND k = ?2"
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let knn: Vec<(String, f32)> = stmt
        .query_map(params![blob, candidate_k], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)? as f32))
        })
        .map_err(be)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(be)?;

    let ids: Vec<String> = knn.iter().map(|(id, _)| id.clone()).collect();
    let rows = load_chunks_by_ids(conn, &ids).map_err(be)?;

    knn.iter()
        .filter_map(|(cid, dist)| {
            let row = rows.iter().find(|r| &r.id == cid)?;
            let score = 1.0 / (1.0 + dist);
            Some(Ok(RetrievedChunk {
                id: ChunkId(cid.clone()),
                document_id: DocumentId(row.document_id.clone()),
                ordinal: row.ordinal,
                external_id: row.external_id.clone(),
                content: query.include_content.then(|| row.content.clone()),
                score,
                primary_score: PrimaryScore::Vector(score),
                chunk_metadata: row.chunk_metadata.clone(),
                document: None,
            }))
        })
        .collect::<RagResult<Vec<_>>>()
}

pub(super) fn retrieve_fts(
    conn: &Connection,
    fts_tbl: &str,
    query_text: &str,
    candidate_k: i64,
    query: &RetrieveQuery,
) -> RagResult<Vec<RetrievedChunk>> {
    let sql = format!(
        "SELECT chunk_id, -rank AS score FROM {fts_tbl}
         WHERE content MATCH ?1
         ORDER BY rank
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let hits: Vec<(String, f32)> = stmt
        .query_map(params![query_text, candidate_k], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)? as f32))
        })
        .map_err(be)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(be)?;

    let ids: Vec<String> = hits.iter().map(|(id, _)| id.clone()).collect();
    let rows = load_chunks_by_ids(conn, &ids).map_err(be)?;

    hits.iter()
        .filter_map(|(cid, score)| {
            let row = rows.iter().find(|r| &r.id == cid)?;
            Some(Ok(RetrievedChunk {
                id: ChunkId(cid.clone()),
                document_id: DocumentId(row.document_id.clone()),
                ordinal: row.ordinal,
                external_id: row.external_id.clone(),
                content: query.include_content.then(|| row.content.clone()),
                score: *score,
                primary_score: PrimaryScore::FullText(*score),
                chunk_metadata: row.chunk_metadata.clone(),
                document: None,
            }))
        })
        .collect::<RagResult<Vec<_>>>()
}

/// Reciprocal-rank-fusion constant (the canonical `k = 60`).
pub(super) const K_RRF: f32 = 60.0;

/// Per-chunk fused scores: `(rrf, vector, full_text, sparse)`.
type FusedScores = (f32, f32, f32, f32);

/// Hybrid retrieval: dense, full-text and (when supplied) sparse hits fused by
/// reciprocal rank.
pub(super) fn retrieve_hybrid(
    conn: &Connection,
    vec_tbl: &str,
    fts_tbl: &str,
    collection: &str,
    candidate_k: i64,
    query: &RetrieveQuery,
) -> RagResult<Vec<RetrievedChunk>> {
    let query_text = query
        .query_text
        .as_ref()
        .ok_or_else(|| RagError::InvalidQuery("hybrid mode requires query_text".to_string()))?;
    let vec_hits = match query.query_vector.as_deref() {
        Some(qv) => dense_hits(conn, vec_tbl, qv, candidate_k)?,
        None => Vec::new(),
    };
    let fts_hits = full_text_hits(conn, fts_tbl, query_text, candidate_k)?;
    let sparse_hits: Vec<(String, f32)> = match query.query_sparse.as_ref() {
        Some(qs) => sparse_scan(conn, collection, qs, candidate_k)?,
        None => Vec::new(),
    };
    let rrf_map = fuse_reciprocal_rank(&vec_hits, &fts_hits, &sparse_hits);

    let mut ranked: Vec<(String, f32, f32, f32, f32)> = rrf_map
        .into_iter()
        .map(|(id, (rrf, vec, fts, sparse))| (id, rrf, vec, fts, sparse))
        .collect();
    truncate_top_k_by(&mut ranked, candidate_k.max(0) as usize, |a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
    });

    let ids: Vec<String> = ranked.iter().map(|(id, ..)| id.clone()).collect();
    let rows = load_chunks_by_ids(conn, &ids).map_err(be)?;
    let rows_by_id: std::collections::HashMap<&str, &ChunkRow> =
        rows.iter().map(|r| (r.id.as_str(), r)).collect();

    ranked
        .iter()
        .filter_map(|(cid, rrf, vec, fts, sparse)| {
            let row = rows_by_id.get(cid.as_str()).copied()?;
            Some(Ok(RetrievedChunk {
                id: ChunkId(cid.clone()),
                document_id: DocumentId(row.document_id.clone()),
                ordinal: row.ordinal,
                external_id: row.external_id.clone(),
                content: query.include_content.then(|| row.content.clone()),
                score: *rrf,
                primary_score: PrimaryScore::Hybrid {
                    vector: *vec,
                    full_text: *fts,
                    sparse: *sparse,
                    rrf: *rrf,
                },
                chunk_metadata: row.chunk_metadata.clone(),
                document: None,
            }))
        })
        .collect::<RagResult<Vec<_>>>()
}

/// Dense KNN hits `(chunk_id, distance)` for `query_vector`.
fn dense_hits(
    conn: &Connection,
    vec_tbl: &str,
    query_vector: &[f32],
    candidate_k: i64,
) -> RagResult<Vec<(String, f32)>> {
    let blob = encode_vec(query_vector);
    let sql = format!(
        "SELECT chunk_id, distance FROM {vec_tbl}
         WHERE embedding MATCH ?1 AND k = ?2"
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    stmt.query_map(params![blob, candidate_k], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)? as f32))
    })
    .map_err(be)?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(be)
}

/// FTS5 hits `(chunk_id, score)` for `query_text`, best first.
fn full_text_hits(
    conn: &Connection,
    fts_tbl: &str,
    query_text: &str,
    candidate_k: i64,
) -> RagResult<Vec<(String, f32)>> {
    let sql = format!(
        "SELECT chunk_id, -rank AS score FROM {fts_tbl}
         WHERE content MATCH ?1
         ORDER BY rank
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    stmt.query_map(params![query_text, candidate_k], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)? as f32))
    })
    .map_err(be)?
    .collect::<rusqlite::Result<Vec<_>>>()
    .map_err(be)
}

/// Reciprocal-rank fusion of the three ranked hit lists, keeping each list's own
/// score alongside the fused one.
fn fuse_reciprocal_rank(
    vec_hits: &[(String, f32)],
    fts_hits: &[(String, f32)],
    sparse_hits: &[(String, f32)],
) -> std::collections::HashMap<String, FusedScores> {
    let mut rrf_map: std::collections::HashMap<String, FusedScores> =
        std::collections::HashMap::new();

    for (rank, (cid, dist)) in vec_hits.iter().enumerate() {
        let vec_score = 1.0 / (1.0 + dist);
        let rrf = 1.0 / (K_RRF + rank as f32 + 1.0);
        let entry = rrf_map.entry(cid.clone()).or_insert((0.0, 0.0, 0.0, 0.0));
        entry.0 += rrf;
        entry.1 = vec_score;
    }
    for (rank, (cid, fts_score)) in fts_hits.iter().enumerate() {
        let rrf = 1.0 / (K_RRF + rank as f32 + 1.0);
        let entry = rrf_map.entry(cid.clone()).or_insert((0.0, 0.0, 0.0, 0.0));
        entry.0 += rrf;
        entry.2 = *fts_score;
    }
    for (rank, (cid, sparse_score)) in sparse_hits.iter().enumerate() {
        let rrf = 1.0 / (K_RRF + rank as f32 + 1.0);
        let entry = rrf_map.entry(cid.clone()).or_insert((0.0, 0.0, 0.0, 0.0));
        entry.0 += rrf;
        entry.3 = *sparse_score;
    }
    rrf_map
}

/// Brute-force sparse scan: score every chunk in `collection` that has a stored
/// sparse embedding against `query_sparse`, returning `(chunk_id, score)` sorted
/// by descending score and capped at `limit`.
///
/// sqlite has no sparse inverted index, so this scans the collection's `sparse`
/// column (skipping `NULL` and any undecodable blob) and computes the merge dot
/// product in Rust — the same approach as the in-memory backend. Adequate for
/// v1 collection sizes; a dedicated inverted index is a follow-up.
pub(super) fn sparse_scan(
    conn: &Connection,
    collection: &str,
    query_sparse: &SparseVector,
    limit: i64,
) -> RagResult<Vec<(String, f32)>> {
    let mut stmt = conn
        .prepare("SELECT id, sparse FROM chunks WHERE collection = ?1 AND sparse IS NOT NULL")
        .map_err(be)?;
    let mut scored: Vec<(String, f32)> = stmt
        .query_map(params![collection], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
        })
        .map_err(be)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(be)?
        .into_iter()
        .filter_map(|(id, blob)| {
            let doc_sparse = decode_sparse(&blob)?;
            let score = sparse_dot(query_sparse, &doc_sparse);
            (score > 0.0).then_some((id, score))
        })
        .collect();
    truncate_top_k_by(&mut scored, limit.max(0) as usize, |a, b| {
        b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(scored)
}

/// Sparse-only retrieval: [`sparse_scan`] then hydrate into `RetrievedChunk`s.
pub(super) fn retrieve_sparse(
    conn: &Connection,
    collection: &str,
    query_sparse: &SparseVector,
    candidate_k: i64,
    query: &RetrieveQuery,
) -> RagResult<Vec<RetrievedChunk>> {
    let scored = sparse_scan(conn, collection, query_sparse, candidate_k)?;
    let ids: Vec<String> = scored.iter().map(|(id, _)| id.clone()).collect();
    let rows = load_chunks_by_ids(conn, &ids).map_err(be)?;
    let rows_by_id: std::collections::HashMap<&str, &ChunkRow> =
        rows.iter().map(|r| (r.id.as_str(), r)).collect();
    scored
        .iter()
        .filter_map(|(cid, score)| {
            let row = rows_by_id.get(cid.as_str()).copied()?;
            Some(Ok(RetrievedChunk {
                id: ChunkId(cid.clone()),
                document_id: DocumentId(row.document_id.clone()),
                ordinal: row.ordinal,
                external_id: row.external_id.clone(),
                content: query.include_content.then(|| row.content.clone()),
                score: *score,
                primary_score: PrimaryScore::Sparse(*score),
                chunk_metadata: row.chunk_metadata.clone(),
                document: None,
            }))
        })
        .collect::<RagResult<Vec<_>>>()
}

/// Late-interaction retrieval: seed a candidate set via dense vector KNN, then
/// rescore each candidate that has a stored multi-vector with MaxSim.
///
/// A first-cut ColBERT retrieval that reuses the dense index for recall and
/// applies late interaction only as a rerank — no per-token index. Candidates
/// without a stored (or decodable) multi-vector are dropped.
pub(super) fn retrieve_late_interaction(
    conn: &Connection,
    vec_tbl: &str,
    query_multi_vector: &MultiVector,
    query_vector: &[f32],
    candidate_k: i64,
    query: &RetrieveQuery,
) -> RagResult<Vec<RetrievedChunk>> {
    let blob = encode_vec(query_vector);
    let sql = format!(
        "SELECT chunk_id FROM {vec_tbl}
         WHERE embedding MATCH ?1 AND k = ?2"
    );
    let mut stmt = conn.prepare(&sql).map_err(be)?;
    let ids: Vec<String> = stmt
        .query_map(params![blob, candidate_k], |r| r.get::<_, String>(0))
        .map_err(be)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(be)?;

    if ids.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let load_sql = format!(
        "SELECT id, document_id, ordinal, external_id, content, multi_vector, chunk_metadata
         FROM chunks WHERE id IN ({placeholders}) AND multi_vector IS NOT NULL"
    );
    let mut load_stmt = conn.prepare(&load_sql).map_err(be)?;
    let mut scored: Vec<RetrievedChunk> = load_stmt
        .query_map(params_from_iter(ids.iter()), |r| {
            let meta_str: String = r.get(6)?;
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, Vec<u8>>(5)?,
                serde_json::from_str(&meta_str).unwrap_or(serde_json::Value::Null),
            ))
        })
        .map_err(be)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(be)?
        .into_iter()
        .filter_map(
            |(id, document_id, ordinal, external_id, content, mv_blob, meta)| {
                let doc_mv = decode_multi_vector(&mv_blob)?;
                let score = max_sim(query_multi_vector, &doc_mv);
                Some(RetrievedChunk {
                    id: ChunkId(id),
                    document_id: DocumentId(document_id),
                    ordinal,
                    external_id,
                    content: query.include_content.then_some(content),
                    score,
                    primary_score: PrimaryScore::LateInteraction(score),
                    chunk_metadata: meta,
                    document: None,
                })
            },
        )
        .collect();
    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(scored)
}

/// Run `query` in its retrieval mode, before filtering and truncation.
pub(super) fn retrieve_mode(
    conn: &Connection,
    vt: &str,
    ft: &str,
    collection: &str,
    candidate_k: i64,
    query: &RetrieveQuery,
) -> RagResult<Vec<RetrievedChunk>> {
    match query.mode {
        RetrieveMode::Vector => {
            let qv = query.query_vector.as_ref().ok_or_else(|| {
                RagError::InvalidQuery(
                    "SQLite backend cannot embed text; supply query_vector".to_string(),
                )
            })?;
            retrieve_vector(conn, vt, qv, candidate_k, query)
        }
        RetrieveMode::FullText => {
            let qt = query.query_text.as_ref().ok_or_else(|| {
                RagError::InvalidQuery("full_text mode requires query_text".to_string())
            })?;
            retrieve_fts(conn, ft, qt, candidate_k, query)
        }
        RetrieveMode::Hybrid => retrieve_hybrid(conn, vt, ft, collection, candidate_k, query),
        RetrieveMode::Sparse => {
            let qs = query.query_sparse.as_ref().ok_or_else(|| {
                RagError::InvalidQuery("sparse mode requires query_sparse".to_string())
            })?;
            retrieve_sparse(conn, collection, qs, candidate_k, query)
        }
        RetrieveMode::LateInteraction => {
            let qmv = query.query_multi_vector.as_ref().ok_or_else(|| {
                RagError::InvalidQuery(
                    "late_interaction mode requires query_multi_vector".to_string(),
                )
            })?;
            let qv = query.query_vector.as_deref().ok_or_else(|| {
                RagError::InvalidQuery(
                    "sqlite backend requires query_vector to seed late_interaction candidates via dense KNN; \
                     query_text alone is insufficient for this backend (the in-memory backend does not have \
                     this requirement)"
                        .to_string(),
                )
            })?;
            retrieve_late_interaction(conn, vt, qmv, qv, candidate_k, query)
        }
    }
}
