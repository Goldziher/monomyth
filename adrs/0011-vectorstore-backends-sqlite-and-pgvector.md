---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# VectorStore backends: SQLite-vec embedded, pgvector for the server (LanceDB rejected)

## Context and Problem Statement

ADR-0008 chose SQLite-vec now, LanceDB later, and deferred pgvector. Since then the deployment shape
has clarified: the embedded engine (desktop/game, single binary) is well served by SQLite-vec, and
the only backend that materially changes the picture is a shared **server** store — which is
Postgres/pgvector, not LanceDB. Which backends do we commit to build?

## Decision Drivers

- The embedded, zero-ops, single-binary case is the common one and already works on SQLite-vec.
- A shared/server deployment needs a networked store multiple clients hit — pgvector fits an
  existing Postgres operational story.
- LanceDB would be a third backend with an extra heavy dependency and no server story.
- `monomyth-knowledge` engine code depends only on `Arc<dyn VectorStore>` (ADR-0007), so backend
  choice is config-only; adding one is a feature-gated trait impl, not a call-site change.

## Considered Options

- SQLite-vec as the default embedded backend + a feature-gated pgvector adapter for the server;
  no LanceDB.
- SQLite-vec now, LanceDB as the scale-up embedded backend, pgvector deferred (ADR-0008 as written).
- Replace SQLite-vec with LanceDB as the default embedded backend.

## Decision Outcome

Chosen option: "SQLite-vec embedded + pgvector server; LanceDB rejected". Keep xberg's built-in
`SqliteVectorStore` as the default, zero-adapter embedded backend. Add a `pgvector` **custom**
`VectorStore` adapter (xberg OSS ships only in-memory + sqlite, so this is a hand-written 9-method
trait impl following the sqlite backend's `Arc<Mutex<…>>` + `spawn_blocking` reference pattern),
feature-gated and off by default, for the shared/server deployment. **LanceDB is rejected**: it buys
little over SQLite-vec at our embedded scale, adds a large dependency, and does not address the
server case that actually motivates a second backend.

This ADR **supersedes ADR-0008**. Scope note: this iteration records the decision and designs the
adapter; the pgvector implementation is the next iteration's work.

### Consequences

- Good, because we keep the zero-ops embedded default and gain a clear, single server backend.
- Good, because dropping LanceDB removes a backend and a heavy dependency we would have carried.
- Good, because the swap stays config-only — engine code sees only `Arc<dyn VectorStore>`.
- Bad, because the pgvector adapter is hand-written (no xberg feature exists) and must track the
  9-method trait; mitigated by the sqlite backend serving as the reference implementation.

### Confirmation

Engine code depends only on `Arc<dyn VectorStore>`; the default build runs on SQLite-vec with no
Postgres dependency. When implemented, the pgvector adapter is selected purely by feature + config,
with no change to any retrieval call site, and is exercised against a test Postgres in CI.

## Pros and Cons of the Options

### LanceDB as embedded backend

- Good, because columnar storage and dataset versioning scale past sqlite-vec eventually.
- Bad, because it is a third backend with a heavy dependency and no server/multi-client story —
  which is the case that actually justifies leaving sqlite.

### ADR-0008 as written (LanceDB later, pgvector deferred)

- Bad, because it prioritized the backend (LanceDB) that our clarified deployment shape does not need
  and deferred the one (pgvector) it does.

## More Information

Supersedes ADR-0008. Builds on ADR-0007 (VectorStore trait as the extension seam). Local ONNX
embeddings (ADR-0009) are the default for the embedded case; hosted embeddings pair naturally with a
pgvector server deployment when that arrives.
