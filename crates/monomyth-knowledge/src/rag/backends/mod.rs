//! Built-in [`VectorStore`](crate::rag::VectorStore) backends.
//!
//! - [`memory`] — pure-Rust brute-force store (default; WASM-safe; tests/dev).
//! - `sqlite` — embedded `rusqlite` + `sqlite-vec` store (feature `sqlite`,
//!   native-only). Heavy/native third-party backends (lancedb, pgvector, …) live
//!   in their own adapter crates, not here.

pub mod memory;

pub mod sqlite;
