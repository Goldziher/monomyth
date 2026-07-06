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
| [0005](./0005-commercial-licensing-ship-reference.md) | Commercial licensing: ship / reference namespaces | accepted |
| [0006](./0006-rust-over-python.md) | Rust for the engine and corpus pipeline | accepted |
| [0007](./0007-adopt-xberg-rag-engine.md) | Adopt xberg as the RAG engine | accepted |
| [0008](./0008-vectorstore-backend-sqlite-then-lancedb.md) | VectorStore backend: SQLite-vec now, LanceDB later | accepted |
| [0009](./0009-local-onnx-embeddings.md) | Local ONNX embeddings | accepted |
