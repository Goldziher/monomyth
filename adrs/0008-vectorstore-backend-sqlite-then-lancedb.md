---
status: superseded by ADR-0011
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

> **Superseded by [ADR-0011](./0011-vectorstore-backends-sqlite-and-pgvector.md)** (2026-07-11):
> the embedded default stays SQLite-vec, but LanceDB is dropped and pgvector is the committed server
> backend. The reasoning below is retained for the historical record.

# VectorStore backend: SQLite-vec now, LanceDB later, pgvector deferred

## Context and Problem Statement

xberg (ADR-0007) is backend-agnostic and ships only in-memory and sqlite-vec stores; LanceDB and
pgvector are the external adapter seam we'd implement. Which backend(s) do we build, and in what
order?

## Decision Drivers

- Get a working end-to-end RAG as fast as possible, with minimal adapter code.
- Fit the self-contained, single-binary engine (embedded, zero-ops) for the common case.
- Keep a path to scale (larger corpora, dataset versioning) and to a shared server later.

## Considered Options

- Use xberg's built-in `SqliteVectorStore` now; add a LanceDB adapter later; defer pgvector.
- Hand-write the LanceDB adapter immediately as the embedded store.
- Implement both LanceDB and pgvector up front.

## Decision Outcome

Chosen option: "SQLite-vec now, LanceDB later, pgvector deferred". Start on xberg's built-in
`SqliteVectorStore` (sqlite-vec KNN + FTS5 + hybrid RRF) — zero adapter code, embedded, hybrid search
out of the box. Implement a `lancedb` feature-gated `VectorStore` adapter when scale / dataset
versioning justify it. A `pgvector` adapter is deferred behind a feature until a shared/server
deployment is real. Engine code depends only on `Arc<dyn VectorStore>`, so the swap is config-only.

### Consequences

- Good, because we reach a working RAG with no backend to write, and hybrid search immediately.
- Good, because adding LanceDB/pgvector later is additive — one trait impl each, no engine changes.
- Bad, because sqlite-vec's scaling ceiling is lower than LanceDB's; acceptable until it isn't.

### Confirmation

The knowledge crate runs on the `sqlite` feature today; the `VectorStore` seam is exercised such that
a future backend requires only a new feature-gated impl, no call-site changes.

## More Information

To add a backend, implement the 9-method async `VectorStore` trait (the sqlite backend's
`Arc<Mutex<…>>` + `spawn_blocking` pattern is the reference). See ADR-0007.
