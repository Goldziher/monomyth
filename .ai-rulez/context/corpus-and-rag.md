---
priority: high
---

# Corpus & RAG (xberg)

monomyth grounds generation in a corpus of comparative mythology, folklore, esoterica, and SF/F.
The RAG layer has two jobs: **distill** the domain schema (the framework artifacts) and **retrieve**
source passages + entities at generation time.

## RAG engine: xberg

We consume the `xberg-rag` crate (crates.io, `1.0.0-rc.*`) — a backend-agnostic RAG layer — plus
`xberg` core for document extraction, chunking, and embeddings. We do **not** fork it.

- **VectorStore trait** (`xberg_rag::VectorStore`): a 9-method async trait over neutral serde types
  (`DocumentRecord`, `ChunkRecord`, `RetrieveQuery`, `Filter`). Inject as `Arc<dyn VectorStore>`.
- **Embedded store — SQLite-vec (now):** use xberg's built-in `SqliteVectorStore` (sqlite-vec KNN
  - FTS5 + hybrid RRF). Zero adapter code. Feature: `sqlite`.
- **LanceDB (later):** implement the `VectorStore` trait as a `lancedb` feature-gated adapter when
  scale / dataset versioning justify it.
- **pgvector (deferred):** a `pgvector` feature-gated adapter for a shared/server deployment.
- **Pipeline:** `xberg_rag::pipeline::{ingest_document, retrieve}` (free functions) drive
  chunk → embed → upsert → query. Use the `Semantic` `ChunkingConfig`.
- **Embeddings:** local ONNX via `CoreEmbedder` (offline, ships in the binary). Feature:
  `pipeline-embeddings`. Implement the `Embedder` trait only if we swap providers.

Consumer features: `vector-store` + `sqlite` + `pipeline` + `pipeline-embeddings`.

## The framework artifacts (`artifacts/frameworks/*.json`)

Hand-encoded, validated analytic frameworks — the schema seed. A 3-tier plot model + character model

- crosswalks. Language-neutral; the Rust `monomyth-core::story` module reads them. Never reformatted
by poly (they are under `artifacts/**`, excluded).

## Licensing model — enforced, not aspirational

Every stored document/chunk carries `namespace` (`ship` | `reference`), `license`, `tier`, and
`domain` in its metadata. This is a **commercial** product, so:

- `ship` = PD / CC0 / CC-BY / CC-BY-SA (share-alike isolated). May be surfaced verbatim.
- `reference` = copyrighted or NonCommercial. Informs generation (structure/priors) but is **never**
  surfaced verbatim.

Enforce the invariant **at retrieval**: any shippable query must filter with the xberg `Filter` IR
(`Filter::Eq("doc.namespace", "ship")`). See the licensing-and-provenance rule.
