//! Embedded SQLite vector store backed by `rusqlite` + `sqlite-vec`.
//!
//! # Architecture
//!
//! `rusqlite::Connection` is `Send` but not `Sync`, and its blocking I/O is
//! incompatible with async runtimes. This backend wraps the connection in
//! `Arc<std::sync::Mutex<Connection>>` and routes every operation through
//! `tokio::task::spawn_blocking`. The `Arc` clone is moved into each blocking
//! task; the `Mutex` is locked inside the task (synchronous context — no lock
//! held across `.await`). This satisfies the `Send + Sync + 'static` bound
//! required by `VectorStore`.
//!
//! # Schema
//!
//! Three persistent tables (`collections`, `documents`, `chunks`) are created
//! once at connection open time. Two per-collection virtual tables —
//! `vec_c{rowid}` (sqlite-vec `vec0`) and `fts_c{rowid}` (FTS5) — are
//! created at `ensure_collection` time. The rowid suffix avoids any collection
//! name character escaping.
//!
//! # sqlite-vec extension
//!
//! sqlite-vec is registered globally once via `sqlite3_auto_extension` so
//! every subsequently opened connection has access to the `vec0` virtual-table
//! module and `vec_*` helper functions.

use crate::rag::capability::Capabilities;
use crate::rag::error::{RagError, RagResult};
use crate::rag::filter::{Filter, FilterField, FilterNamespace};
use crate::rag::query::{RetrieveMode, RetrieveOutput, RetrieveQuery};
use crate::rag::scoring::{max_sim, sparse_dot};
use crate::rag::store::VectorStore;
use crate::rag::types::{
    ChunkId, ChunkRecord, CollectionSpec, CollectionStats, DistanceMetric, DocumentId,
    DocumentRecord, DocumentSummary, IndexMethod, MultiVector, PrimaryScore, RetrievedChunk,
    SparseVector, validate_chunk_side_vectors,
};
use async_trait::async_trait;
use rusqlite::{Connection, params, params_from_iter};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::instrument;

mod codec;
mod filter_eval;
mod retrieve;

use codec::{
    decode_multi_vector, decode_sparse, encode_multi_vector, encode_sparse, encode_vec,
    index_method_str, metric_str, parse_index_method, parse_metric,
};
use filter_eval::eval_filter;
use retrieve::{
    filter_chunks, retrieve_fts, retrieve_hybrid, retrieve_late_interaction, retrieve_sparse,
    retrieve_vector,
};

/// Register the sqlite-vec extension with SQLite's auto-extension mechanism.
///
/// Called once before the first `Connection::open*`. Every subsequent connection
/// opened in this process will have the `vec0` virtual-table module and
/// `vec_*` scalar functions available automatically.
#[allow(unsafe_code)]
fn register_sqlite_vec_once() {
    static INIT: std::sync::Once = std::sync::Once::new();
    // SAFETY: `sqlite_vec::sqlite3_vec_init` has the C ABI SQLite's auto-extension ~keep
    // mechanism expects (`sqlite3_auto_extension`'s documented entry-point signature), ~keep
    // so the transmute from function pointer to `AutoExtFn` is a same-signature cast, ~keep
    // not a type change. `Once::call_once` guarantees `sqlite3_auto_extension` — which ~keep
    // is itself safe to call concurrently per SQLite's own thread-safety contract — runs ~keep
    // exactly once per process, matching the "register before the first `Connection::open*`" ~keep
    // precondition documented above. ~keep
    INIT.call_once(|| unsafe {
        type AutoExtFn = unsafe extern "C" fn(
            *mut rusqlite::ffi::sqlite3,
            *mut *mut std::ffi::c_char,
            *const rusqlite::ffi::sqlite3_api_routines,
        ) -> std::ffi::c_int;
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute::<*const (), AutoExtFn>(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

fn be(e: rusqlite::Error) -> RagError {
    RagError::Backend(Box::new(e))
}

fn io_be(msg: &str) -> RagError {
    RagError::Backend(Box::new(std::io::Error::other(msg.to_string())))
}

/// Truncate `items` to the top `k` elements by `cmp`, in the same order a full
/// `sort_by(cmp)` followed by `truncate(k)` would produce.
///
/// When `k` is smaller than `items.len()`, partitions with
/// [`slice::select_nth_unstable_by`] (`O(n)`) to isolate the top-k elements
/// before sorting only that k-slice (`O(k log k)`), instead of fully sorting
/// all `n` elements (`O(n log n)`). Falls back to a plain sort when `k` is not
/// smaller than the input (nothing to save) or is `0` (nothing to sort).
fn truncate_top_k_by<T, F>(items: &mut Vec<T>, k: usize, mut cmp: F)
where
    F: FnMut(&T, &T) -> std::cmp::Ordering,
{
    if k < items.len() {
        if k > 0 {
            items.select_nth_unstable_by(k - 1, &mut cmp);
        }
        items.truncate(k);
    }
    items.sort_by(cmp);
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn vec_table(rowid: i64) -> String {
    format!("vec_c{rowid}")
}

fn fts_table(rowid: i64) -> String {
    format!("fts_c{rowid}")
}

const BASE_SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS collections (
    name           TEXT PRIMARY KEY,
    embedding_dim  INTEGER NOT NULL,
    distance_metric TEXT NOT NULL DEFAULT 'cosine',
    index_method   TEXT NOT NULL DEFAULT 'flat'
) STRICT;

CREATE TABLE IF NOT EXISTS documents (
    id          TEXT PRIMARY KEY,
    collection  TEXT NOT NULL,
    external_id TEXT,
    title       TEXT,
    mime        TEXT,
    source_uri  TEXT,
    full_text   TEXT NOT NULL DEFAULT '',
    keywords    TEXT NOT NULL DEFAULT '[]',
    entities    TEXT NOT NULL DEFAULT 'null',
    labels      TEXT NOT NULL DEFAULT 'null',
    metadata    TEXT NOT NULL DEFAULT 'null',
    ingested_at INTEGER NOT NULL
) STRICT;

CREATE UNIQUE INDEX IF NOT EXISTS idx_docs_ext
    ON documents(collection, external_id)
    WHERE external_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_docs_coll ON documents(collection);

CREATE TABLE IF NOT EXISTS chunks (
    id             TEXT PRIMARY KEY,
    document_id    TEXT NOT NULL,
    collection     TEXT NOT NULL,
    ordinal        INTEGER NOT NULL,
    external_id    TEXT,
    content        TEXT NOT NULL,
    embedding      BLOB NOT NULL,
    sparse         BLOB,
    multi_vector   BLOB,
    chunk_metadata TEXT NOT NULL DEFAULT 'null'
) STRICT;

CREATE INDEX IF NOT EXISTS idx_chunks_doc  ON chunks(document_id);
CREATE INDEX IF NOT EXISTS idx_chunks_coll ON chunks(collection);
";

fn setup_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(BASE_SCHEMA)?;
    migrate_chunk_side_vector_columns(conn)
}

/// Add the `sparse` / `multi_vector` columns to a pre-existing `chunks` table.
///
/// `BASE_SCHEMA`'s `CREATE TABLE IF NOT EXISTS` provides these columns on a
/// fresh database but is a no-op against a database created before they were
/// introduced. This backfills them idempotently: it inspects `PRAGMA
/// table_info` and only issues an `ALTER TABLE ADD COLUMN` for a column that is
/// actually missing (both are nullable, so existing rows default to `NULL`).
fn migrate_chunk_side_vector_columns(conn: &Connection) -> rusqlite::Result<()> {
    let existing: std::collections::HashSet<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(chunks)")?;
        let cols = stmt.query_map([], |row| row.get::<_, String>(1))?;
        cols.collect::<rusqlite::Result<_>>()?
    };
    if !existing.contains("sparse") {
        conn.execute_batch("ALTER TABLE chunks ADD COLUMN sparse BLOB;")?;
    }
    if !existing.contains("multi_vector") {
        conn.execute_batch("ALTER TABLE chunks ADD COLUMN multi_vector BLOB;")?;
    }
    Ok(())
}

fn setup_pragmas(conn: &Connection, in_memory: bool) -> rusqlite::Result<()> {
    if in_memory {
        conn.execute_batch("PRAGMA foreign_keys = ON;")
    } else {
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous   = NORMAL;
             PRAGMA foreign_keys  = ON;",
        )
    }
}

struct CollectionRow {
    rowid: i64,
    spec: CollectionSpec,
}

fn get_collection_row(conn: &Connection, name: &str) -> rusqlite::Result<Option<CollectionRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT rowid, name, embedding_dim, distance_metric, index_method
         FROM collections WHERE name = ?1",
    )?;
    match stmt.query_row(params![name], |row| {
        Ok(CollectionRow {
            rowid: row.get(0)?,
            spec: CollectionSpec {
                name: row.get(1)?,
                embedding_dim: row.get::<_, u32>(2)?,
                distance_metric: parse_metric(&row.get::<_, String>(3)?),
                index_method: parse_index_method(&row.get::<_, String>(4)?),
            },
        })
    }) {
        Ok(r) => Ok(Some(r)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

struct ChunkRow {
    id: String,
    document_id: String,
    ordinal: u32,
    external_id: Option<String>,
    content: String,
    chunk_metadata: serde_json::Value,
}

fn load_chunks_by_ids(conn: &Connection, ids: &[String]) -> rusqlite::Result<Vec<ChunkRow>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", ids.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!(
        "SELECT id, document_id, ordinal, external_id, content, chunk_metadata
         FROM chunks WHERE id IN ({placeholders})"
    );
    let mut stmt = conn.prepare(&sql)?;
    stmt.query_map(params_from_iter(ids.iter()), |row| {
        let meta_str: String = row.get(5)?;
        Ok(ChunkRow {
            id: row.get(0)?,
            document_id: row.get(1)?,
            ordinal: row.get::<_, u32>(2)?,
            external_id: row.get(3)?,
            content: row.get(4)?,
            chunk_metadata: serde_json::from_str(&meta_str).unwrap_or(serde_json::Value::Null),
        })
    })?
    .collect()
}

fn load_document_record(
    conn: &Connection,
    document_id: &str,
) -> rusqlite::Result<Option<(DocumentRecord, i64)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT external_id, title, mime, source_uri, full_text,
                keywords, entities, labels, metadata, ingested_at
         FROM documents WHERE id = ?1",
    )?;
    match stmt.query_row(params![document_id], |row| {
        let kw_str: String = row.get(5)?;
        let ent_str: String = row.get(6)?;
        let lbl_str: String = row.get(7)?;
        let meta_str: String = row.get(8)?;
        let ingested_at: i64 = row.get(9)?;
        Ok((
            DocumentRecord {
                external_id: row.get(0)?,
                title: row.get(1)?,
                mime: row.get(2)?,
                source_uri: row.get(3)?,
                full_text: row.get(4)?,
                keywords: serde_json::from_str(&kw_str).unwrap_or_default(),
                entities: serde_json::from_str(&ent_str).unwrap_or(serde_json::Value::Null),
                labels: serde_json::from_str(&lbl_str).unwrap_or(serde_json::Value::Null),
                metadata: serde_json::from_str(&meta_str).unwrap_or(serde_json::Value::Null),
            },
            ingested_at,
        ))
    }) {
        Ok(r) => Ok(Some(r)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

fn doc_to_summary(id: &DocumentId, rec: &DocumentRecord, ingested_at: i64) -> DocumentSummary {
    DocumentSummary {
        id: id.clone(),
        external_id: rec.external_id.clone(),
        title: rec.title.clone(),
        mime: rec.mime.clone(),
        keywords: rec.keywords.clone(),
        labels: rec.labels.clone(),
        entities: rec.entities.clone(),
        metadata: rec.metadata.clone(),
        ingested_at: Some(ingested_at).filter(|&t| t > 0),
    }
}

/// Run `f` inside a `BEGIN IMMEDIATE … COMMIT` transaction.
///
/// On error, rolls back and returns the error. Uses `&Connection` (not
/// `&mut Connection`) so it can be called inside the read-locked `with_conn`
/// closure without requiring exclusive access.
fn with_tx<F, T>(conn: &Connection, f: F) -> RagResult<T>
where
    F: FnOnce() -> RagResult<T>,
{
    conn.execute_batch("BEGIN IMMEDIATE;").map_err(be)?;
    match f() {
        Ok(v) => {
            conn.execute_batch("COMMIT;").map_err(be)?;
            Ok(v)
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK;");
            Err(e)
        }
    }
}

/// An embedded vector store backed by SQLite (`rusqlite`) + the `sqlite-vec`
/// vector-search extension.
///
/// Supports vector KNN (via `vec0`), full-text BM25 (via FTS5), and hybrid
/// retrieval with reciprocal rank fusion. All filtering is evaluated in Rust
/// after SQL retrieval.
///
/// # Thread safety
///
/// The underlying `rusqlite::Connection` is wrapped in
/// `Arc<std::sync::Mutex<Connection>>`. Every trait method routes through
/// `tokio::task::spawn_blocking`, locking the mutex inside the blocking task so
/// no lock is ever held across an `.await` point.
pub struct SqliteVectorStore {
    name: String,
    conn: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for SqliteVectorStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `rusqlite::Connection` does not implement `Debug`, so the store is ~keep
        // summarized by its name rather than its connection internals. ~keep
        formatter
            .debug_struct("SqliteVectorStore")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl SqliteVectorStore {
    fn open_conn(path: Option<&str>) -> RagResult<Connection> {
        let in_memory = path.is_none();
        let conn = match path {
            None => Connection::open_in_memory().map_err(be)?,
            Some(p) => Connection::open(p).map_err(be)?,
        };
        setup_pragmas(&conn, in_memory).map_err(be)?;
        setup_schema(&conn).map_err(be)?;
        Ok(conn)
    }

    /// Open an in-memory store (contents are lost when dropped).
    pub async fn open_in_memory(name: impl Into<String> + Send + 'static) -> RagResult<Self> {
        register_sqlite_vec_once();
        tokio::task::spawn_blocking(move || {
            let conn = Self::open_conn(None)?;
            Ok(Self {
                name: name.into(),
                conn: Arc::new(Mutex::new(conn)),
            })
        })
        .await
        .map_err(|e| io_be(&e.to_string()))?
    }

    /// Open a file-backed store at `path` (created if absent).
    pub async fn open(
        name: impl Into<String> + Send + 'static,
        path: impl Into<String> + Send + 'static,
    ) -> RagResult<Self> {
        register_sqlite_vec_once();
        let path = path.into();
        tokio::task::spawn_blocking(move || {
            let conn = Self::open_conn(Some(&path))?;
            Ok(Self {
                name: name.into(),
                conn: Arc::new(Mutex::new(conn)),
            })
        })
        .await
        .map_err(|e| io_be(&e.to_string()))?
    }

    async fn with_conn<F, T>(&self, f: F) -> RagResult<T>
    where
        F: FnOnce(&Connection) -> RagResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn
                .lock()
                .map_err(|_| io_be("SQLite connection mutex poisoned"))?;
            f(&guard)
        })
        .await
        .map_err(|e| io_be(&e.to_string()))?
    }
}

#[async_trait]
impl VectorStore for SqliteVectorStore {
    fn name(&self) -> &str {
        &self.name
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            full_text: true,
            hybrid: true,
            filtering: true,
            sparse: true,
            late_interaction: true,
            index_methods: vec![IndexMethod::Flat],
        }
    }

    #[instrument(skip(self, spec), fields(store = %self.name, collection = %spec.name))]
    async fn ensure_collection(&self, spec: &CollectionSpec) -> RagResult<()> {
        let spec = spec.clone();
        self.with_conn(move |conn| {
            let existing = get_collection_row(conn, &spec.name).map_err(be)?;
            match existing {
                Some(row) if row.spec.embedding_dim != spec.embedding_dim => {
                    Err(RagError::CollectionAlreadyExists(spec.name))
                }
                Some(_) => Ok(()),
                None => with_tx(conn, || {
                    let dim = spec.embedding_dim;
                    let m = metric_str(spec.distance_metric);
                    let idx = index_method_str(spec.index_method);
                    conn.execute(
                        "INSERT INTO collections (name, embedding_dim, distance_metric, index_method)
                             VALUES (?1, ?2, ?3, ?4)",
                        params![spec.name, dim, m, idx],
                    )
                    .map_err(be)?;
                    let rowid = conn.last_insert_rowid();
                    let vt = vec_table(rowid);
                    let ft = fts_table(rowid);
                    conn.execute_batch(&format!(
                        "CREATE VIRTUAL TABLE IF NOT EXISTS {vt} USING vec0(
                                 chunk_id TEXT PRIMARY KEY,
                                 embedding float[{dim}] distance_metric={m}
                             );
                             CREATE VIRTUAL TABLE IF NOT EXISTS {ft} USING fts5(
                                 chunk_id UNINDEXED,
                                 content
                             );"
                    ))
                    .map_err(be)?;
                    Ok(())
                }),
            }
        })
        .await
    }

    #[instrument(skip(self), fields(store = %self.name, collection = %collection))]
    async fn drop_collection(&self, collection: &str) -> RagResult<()> {
        let collection = collection.to_string();
        self.with_conn(move |conn| {
            let row = get_collection_row(conn, &collection)
                .map_err(be)?
                .ok_or_else(|| RagError::CollectionNotFound(collection.clone()))?;

            with_tx(conn, || {
                let vt = vec_table(row.rowid);
                let ft = fts_table(row.rowid);
                conn.execute_batch(&format!(
                    "DROP TABLE IF EXISTS {vt};
                     DROP TABLE IF EXISTS {ft};"
                ))
                .map_err(be)?;
                conn.execute(
                    "DELETE FROM chunks    WHERE collection = ?1",
                    params![collection],
                )
                .map_err(be)?;
                conn.execute(
                    "DELETE FROM documents WHERE collection = ?1",
                    params![collection],
                )
                .map_err(be)?;
                conn.execute(
                    "DELETE FROM collections WHERE name = ?1",
                    params![collection],
                )
                .map_err(be)?;
                Ok(())
            })
        })
        .await
    }

    #[instrument(skip(self), fields(store = %self.name, collection = %collection))]
    async fn get_collection(&self, collection: &str) -> RagResult<Option<CollectionSpec>> {
        let collection = collection.to_string();
        self.with_conn(move |conn| {
            Ok(get_collection_row(conn, &collection)
                .map_err(be)?
                .map(|r| r.spec))
        })
        .await
    }

    #[instrument(skip(self, document, chunks), fields(store = %self.name, collection = %collection))]
    async fn upsert_document(
        &self,
        collection: &str,
        document: &DocumentRecord,
        chunks: &[ChunkRecord],
    ) -> RagResult<DocumentId> {
        let collection = collection.to_string();
        let document = document.clone();
        let chunks = chunks.to_vec();
        let store_name = self.name.clone();

        self.with_conn(move |conn| {
            let row = get_collection_row(conn, &collection)
                .map_err(be)?
                .ok_or_else(|| RagError::CollectionNotFound(collection.clone()))?;
            let dim = row.spec.embedding_dim;

            for chunk in &chunks {
                if chunk.embedding.len() as u32 != dim {
                    return Err(RagError::EmbeddingDimMismatch {
                        expected: dim,
                        got: chunk.embedding.len() as u32,
                    });
                }
                validate_chunk_side_vectors(chunk)?;
            }

            let vt = vec_table(row.rowid);
            let ft = fts_table(row.rowid);

            with_tx(conn, || {
                let doc_id: DocumentId = match document.external_id.as_deref() {
                    Some(ext) => {
                        let mut stmt = conn
                            .prepare_cached("SELECT id FROM documents WHERE collection = ?1 AND external_id = ?2")
                            .map_err(be)?;
                        match stmt.query_row(params![collection, ext], |r| r.get::<_, String>(0)) {
                            Ok(id) => {
                                delete_chunks_for_doc(conn, &id, &vt, &ft)?;
                                DocumentId(id)
                            }
                            Err(rusqlite::Error::QueryReturnedNoRows) => {
                                insert_new_document(conn, &collection, &document, &store_name)?
                            }
                            Err(e) => return Err(be(e)),
                        }
                    }
                    None => insert_new_document(conn, &collection, &document, &store_name)?,
                };

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

                for chunk in &chunks {
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
                }

                Ok(doc_id)
            })
        })
        .await
    }

    #[instrument(skip(self, ids), fields(store = %self.name, collection = %collection))]
    async fn delete_documents(&self, collection: &str, ids: &[DocumentId]) -> RagResult<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let collection = collection.to_string();
        let ids = ids.to_vec();

        self.with_conn(move |conn| {
            let row = get_collection_row(conn, &collection)
                .map_err(be)?
                .ok_or_else(|| RagError::CollectionNotFound(collection.clone()))?;
            let vt = vec_table(row.rowid);
            let ft = fts_table(row.rowid);

            with_tx(conn, || {
                let mut removed = 0u64;
                for id in &ids {
                    let n = delete_chunks_for_doc(conn, &id.0, &vt, &ft)?;
                    let del = conn
                        .execute("DELETE FROM documents WHERE id = ?1", params![id.0])
                        .map_err(be)?;
                    if del > 0 || n > 0 {
                        removed += 1;
                    }
                }
                Ok(removed)
            })
        })
        .await
    }

    #[instrument(skip(self, filter), fields(store = %self.name, collection = %collection))]
    async fn delete_by_filter(&self, collection: &str, filter: &Filter) -> RagResult<u64> {
        filter.validate()?;
        let collection = collection.to_string();
        let filter = filter.clone();

        self.with_conn(move |conn| {
            let row = get_collection_row(conn, &collection)
                .map_err(be)?
                .ok_or_else(|| RagError::CollectionNotFound(collection.clone()))?;
            let vt = vec_table(row.rowid);
            let ft = fts_table(row.rowid);

            let doc_ids: Vec<String> = {
                let mut stmt = conn
                    .prepare_cached("SELECT id FROM documents WHERE collection = ?1")
                    .map_err(be)?;
                stmt.query_map(params![collection], |r| r.get(0))
                    .map_err(be)?
                    .collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(be)?
            };

            let mut to_delete: Vec<String> = Vec::new();
            for doc_id in &doc_ids {
                let Some((doc_rec, _)) = load_document_record(conn, doc_id).map_err(be)? else {
                    continue;
                };
                let chunk_ids: Vec<String> = {
                    let mut stmt = conn
                        .prepare_cached(
                            "SELECT id, ordinal, external_id, content, embedding, chunk_metadata
                             FROM chunks WHERE document_id = ?1",
                        )
                        .map_err(be)?;
                    stmt.query_map(params![doc_id], |r| r.get::<_, String>(0))
                        .map_err(be)?
                        .collect::<rusqlite::Result<Vec<_>>>()
                        .map_err(be)?
                };
                let matches = chunk_ids.iter().any(|cid| {
                    conn.query_row(
                        "SELECT ordinal, external_id, content, embedding, chunk_metadata
                         FROM chunks WHERE id = ?1",
                        params![cid],
                        |r| {
                            let meta_str: String = r.get(4)?;
                            Ok(ChunkRecord {
                                external_id: r.get(1)?,
                                ordinal: r.get::<_, u32>(0)?,
                                content: r.get(2)?,
                                embedding: Vec::new(),
                                sparse_embedding: None,
                                multi_vector: None,
                                chunk_metadata: serde_json::from_str(&meta_str)
                                    .unwrap_or(serde_json::Value::Null),
                            })
                        },
                    )
                    .map(|chunk| eval_filter(&filter, &doc_rec, &chunk))
                    .unwrap_or(false)
                });
                if matches {
                    to_delete.push(doc_id.clone());
                }
            }

            if to_delete.is_empty() {
                return Ok(0);
            }

            with_tx(conn, || {
                let mut removed = 0u64;
                for doc_id in &to_delete {
                    delete_chunks_for_doc(conn, doc_id, &vt, &ft)?;
                    let n = conn
                        .execute("DELETE FROM documents WHERE id = ?1", params![doc_id])
                        .map_err(be)?;
                    removed += n as u64;
                }
                Ok(removed)
            })
        })
        .await
    }

    #[instrument(skip(self, query), fields(store = %self.name, collection = %collection, mode = ?query.mode))]
    async fn retrieve(&self, collection: &str, query: &RetrieveQuery) -> RagResult<RetrieveOutput> {
        let collection = collection.to_string();
        let query = query.clone();

        self.with_conn(move |conn| {
            let row = get_collection_row(conn, &collection)
                .map_err(be)?
                .ok_or_else(|| RagError::CollectionNotFound(collection.clone()))?;
            let spec = &row.spec;

            query.validate(spec)?;

            let vt = vec_table(row.rowid);
            let ft = fts_table(row.rowid);
            let candidate_k = query.top_k as i64 * query.candidate_multiplier.unwrap_or(4).max(1) as i64;

            let t0 = std::time::Instant::now();
            let mode = query.mode;

            let mut chunks: Vec<RetrievedChunk> = match mode {
                RetrieveMode::Vector => {
                    let qv = query.query_vector.as_ref().ok_or_else(|| {
                        RagError::InvalidQuery("SQLite backend cannot embed text; supply query_vector".to_string())
                    })?;
                    retrieve_vector(conn, &vt, qv, candidate_k, &query)?
                }
                RetrieveMode::FullText => {
                    let qt = query
                        .query_text
                        .as_ref()
                        .ok_or_else(|| RagError::InvalidQuery("full_text mode requires query_text".to_string()))?;
                    retrieve_fts(conn, &ft, qt, candidate_k, &query)?
                }
                RetrieveMode::Hybrid => {
                    let qt = query
                        .query_text
                        .as_ref()
                        .ok_or_else(|| RagError::InvalidQuery("hybrid mode requires query_text".to_string()))?;
                    let qv = query.query_vector.as_deref();
                    let qs = query.query_sparse.as_ref();
                    retrieve_hybrid(conn, &vt, &ft, qt, qv, qs, &collection, candidate_k, &query)?
                }
                RetrieveMode::Sparse => {
                    let qs = query
                        .query_sparse
                        .as_ref()
                        .ok_or_else(|| RagError::InvalidQuery("sparse mode requires query_sparse".to_string()))?;
                    retrieve_sparse(conn, &collection, qs, candidate_k, &query)?
                }
                RetrieveMode::LateInteraction => {
                    let qmv = query.query_multi_vector.as_ref().ok_or_else(|| {
                        RagError::InvalidQuery("late_interaction mode requires query_multi_vector".to_string())
                    })?;
                    let qv = query.query_vector.as_deref().ok_or_else(|| {
                        RagError::InvalidQuery(
                            "sqlite backend requires query_vector to seed late_interaction candidates via dense KNN; \
                             query_text alone is insufficient for this backend (the in-memory backend does not have \
                             this requirement)"
                                .to_string(),
                        )
                    })?;
                    retrieve_late_interaction(conn, &vt, qmv, qv, candidate_k, &query)?
                }
            };

            let primary_latency_ms = t0.elapsed().as_millis() as u64;

            if let Some(ref filter) = query.filter {
                chunks = filter_chunks(conn, chunks, filter)?;
            }

            if query.include_document {
                for chunk in &mut chunks {
                    if let Some((rec, iat)) = load_document_record(conn, &chunk.document_id.0).map_err(be)? {
                        chunk.document = Some(doc_to_summary(&chunk.document_id, &rec, iat));
                    }
                }
            }

            if query.group_by_document {
                let mut seen: std::collections::HashMap<DocumentId, ()> = std::collections::HashMap::new();
                chunks.retain(|c| seen.insert(c.document_id.clone(), ()).is_none());
            }

            chunks.truncate(query.top_k as usize);

            Ok(RetrieveOutput {
                mode,
                chunks,
                primary_latency_ms,
            })
        })
        .await
    }

    #[instrument(skip(self), fields(store = %self.name, collection = %collection))]
    async fn collection_stats(&self, collection: &str) -> RagResult<CollectionStats> {
        let collection = collection.to_string();
        self.with_conn(move |conn| {
            get_collection_row(conn, &collection)
                .map_err(be)?
                .ok_or_else(|| RagError::CollectionNotFound(collection.clone()))?;

            let documents: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM documents WHERE collection = ?1",
                    params![collection],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(be)? as u64;

            let chunks: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM chunks WHERE collection = ?1",
                    params![collection],
                    |r| r.get::<_, i64>(0),
                )
                .map_err(be)? as u64;

            let last_ingested_at: Option<i64> = conn
                .query_row(
                    "SELECT MAX(ingested_at) FROM documents WHERE collection = ?1",
                    params![collection],
                    |r| r.get(0),
                )
                .map_err(be)?;

            Ok(CollectionStats {
                documents,
                chunks,
                last_ingested_at: last_ingested_at.filter(|&t| t > 0),
            })
        })
        .await
    }
}

/// Insert a new document row with a rowid-derived id and return the id.
fn insert_new_document(
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
fn delete_chunks_for_doc(
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

#[cfg(test)]
mod tests;
