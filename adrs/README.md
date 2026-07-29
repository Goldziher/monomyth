# Architecture Decision Records

This directory records the significant architecture decisions for monomyth, in
[MADR](https://adr.github.io/madr/) format (Markdown Any Decision Records).

## Why

An ADR captures a single decision — its context, the options weighed, the choice, and the
consequences — at the time it was made. It is a durable answer to "why is it built this way?"
that survives long after the conversation is forgotten. ADRs are immutable once accepted; a
changed decision is a *new* ADR that supersedes the old one (never an edit).

## Conventions

- One decision per file, named `NNNN-kebab-title.md` (zero-padded, monotonically increasing).
- Start from [`template.md`](./template.md).
- `status` lifecycle: `proposed` → `accepted` → (later) `deprecated` / `superseded by ADR-NNNN`.
- Keep it short. Link related ADRs rather than repeating them.

## Index

| ADR | Title | Status |
|-----|-------|--------|
| [0001](./0001-engine-plus-pluggable-frontends.md) | Engine + pluggable frontends | accepted |
| [0002](./0002-hybrid-generation.md) | Hybrid generation (procedural + LLM) | accepted |
| [0003](./0003-slotmap-graph-domain-model.md) | Slotmap graph domain model (not ECS) | accepted |
| [0004](./0004-mythology-frameworks-as-domain-schema.md) | Comparative-mythology frameworks as the domain schema | accepted |
| [0005](./0005-commercial-licensing-ship-reference.md) | Commercial licensing: ship / reference namespaces | accepted (commercial framing re-motivated by [0030](./0030-mit-license-and-experimental-status.md)) |
| [0006](./0006-rust-over-python.md) | Rust for the engine and corpus pipeline | accepted |
| [0007](./0007-adopt-xberg-rag-engine.md) | Adopt xberg as the RAG engine | accepted |
| [0008](./0008-vectorstore-backend-sqlite-then-lancedb.md) | VectorStore backend: SQLite-vec now, LanceDB later | superseded by [0011](./0011-vectorstore-backends-sqlite-and-pgvector.md) |
| [0009](./0009-local-onnx-embeddings.md) | Local ONNX embeddings | accepted |
| [0010](./0010-rust-corpus-acquisition-pipeline.md) | Rust corpus acquisition pipeline and stored provenance | accepted (storage mechanism amended by [0012](./0012-opendal-corpus-blob-storage.md)) |
| [0011](./0011-vectorstore-backends-sqlite-and-pgvector.md) | VectorStore backends: SQLite-vec embedded, pgvector server | accepted |
| [0012](./0012-opendal-corpus-blob-storage.md) | OpenDAL corpus blob storage (local FS + cloud buckets) | accepted |
| [0013](./0013-trait-first-planes-architecture.md) | Trait-first, planes-over-layers architecture | accepted |
| [0014](./0014-contract-as-extraction-generation-pivot.md) | The contract as the extraction⇄generation pivot | accepted |
| [0015](./0015-layered-override-configuration.md) | Layered override configuration (monomyth-config) | accepted |
| [0016](./0016-build-time-methodology-and-law-synthesis.md) | Build-time methodology + reference-ingest law synthesis | accepted |
| [0017](./0017-genre-as-config-dimension.md) | Genre as a cross-cutting config dimension | accepted |
| [0018](./0018-extraction-subsystem.md) | Extraction subsystem (monomyth-extract) | accepted |
| [0019](./0019-user-uploaded-media-namespace.md) | User-uploaded media + user namespace/tier | accepted |
| [0020](./0020-medium-agnostic-rendering-adapters.md) | Medium-agnostic rendering adapters | accepted |
| [0021](./0021-rust-architecture-guidelines-in-ai-rulez.md) | Rust architecture guidelines authored in ai-rulez | accepted |
| [0022](./0022-scored-weighted-classification-attributes.md) | Scored / weighted classification attributes | accepted |
| [0023](./0023-benchmark-driven-evaluation.md) | Benchmark-driven evaluation + ground-truth corpus | accepted |
| [0024](./0024-llm-judge-feedback-loop-synthesis.md) | LLM-as-judge feedback loop for reference→law synthesis | accepted |
| [0025](./0025-reference-retrieval-quality.md) | Reference retrieval quality: dedup, multi-query coverage, hybrid RRF | accepted |
| [0026](./0026-reference-path-first-rag-enrichment.md) | Reference-path-first RAG enrichment (keywords + entities) | accepted |
| [0027](./0027-long-form-compose-pipeline.md) | Long-form compose pipeline | accepted (realized: all four slices shipped) |
| [0028](./0028-genre-classifier-in-genre-crate.md) | GenreClassifier seam in monomyth-genre (contracts-edge exception) | accepted |
| [0029](./0029-quick-xml-advisory-posture.md) | quick-xml advisory posture under opendal services-fs | accepted |
| [0030](./0030-mit-license-and-experimental-status.md) | MIT license and experimental status | accepted |
