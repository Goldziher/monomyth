---
title: Architecture decisions
description: The ADR log — every significant architecture decision, in MADR format, with context and consequences.
---

monomyth records its significant architecture decisions as ADRs (Architecture Decision Records) in
[MADR](https://adr.github.io/madr/) format under
[`adrs/`](https://github.com/Goldziher/monomyth/tree/main/adrs). An ADR captures a single decision —
its context, the options weighed, the choice, and the consequences — at the time it was made. ADRs
are immutable once accepted; a changed decision is a *new* ADR that supersedes the old one, never an
edit.

## Foundations

| ADR | Decision |
|---|---|
| [0001](https://github.com/Goldziher/monomyth/blob/main/adrs/0001-engine-plus-pluggable-frontends.md) | Engine + pluggable frontends |
| [0002](https://github.com/Goldziher/monomyth/blob/main/adrs/0002-hybrid-generation.md) | Hybrid generation (procedural + LLM) |
| [0003](https://github.com/Goldziher/monomyth/blob/main/adrs/0003-slotmap-graph-domain-model.md) | Slotmap graph domain model (not ECS) |
| [0004](https://github.com/Goldziher/monomyth/blob/main/adrs/0004-mythology-frameworks-as-domain-schema.md) | Comparative-mythology frameworks as the domain schema |
| [0006](https://github.com/Goldziher/monomyth/blob/main/adrs/0006-rust-over-python.md) | Rust for the engine and corpus pipeline |

## Corpus, licensing & storage

| ADR | Decision |
|---|---|
| [0005](https://github.com/Goldziher/monomyth/blob/main/adrs/0005-commercial-licensing-ship-reference.md) | Commercial licensing: ship / reference namespaces |
| [0007](https://github.com/Goldziher/monomyth/blob/main/adrs/0007-adopt-xberg-rag-engine.md) | Adopt xberg as the RAG engine |
| [0008](https://github.com/Goldziher/monomyth/blob/main/adrs/0008-vectorstore-backend-sqlite-then-lancedb.md) | VectorStore backend: SQLite-vec now, LanceDB later — superseded by [0011](https://github.com/Goldziher/monomyth/blob/main/adrs/0011-vectorstore-backends-sqlite-and-pgvector.md) |
| [0009](https://github.com/Goldziher/monomyth/blob/main/adrs/0009-local-onnx-embeddings.md) | Local ONNX embeddings |
| [0010](https://github.com/Goldziher/monomyth/blob/main/adrs/0010-rust-corpus-acquisition-pipeline.md) | Rust corpus acquisition pipeline and stored provenance — storage mechanism amended by [0012](https://github.com/Goldziher/monomyth/blob/main/adrs/0012-opendal-corpus-blob-storage.md) |
| [0011](https://github.com/Goldziher/monomyth/blob/main/adrs/0011-vectorstore-backends-sqlite-and-pgvector.md) | VectorStore backends: SQLite-vec embedded, pgvector server |
| [0012](https://github.com/Goldziher/monomyth/blob/main/adrs/0012-opendal-corpus-blob-storage.md) | OpenDAL corpus blob storage (local FS + cloud buckets) |
| [0029](https://github.com/Goldziher/monomyth/blob/main/adrs/0029-quick-xml-advisory-posture.md) | quick-xml advisory posture under opendal services-fs |

## Planes, config & genre

| ADR | Decision |
|---|---|
| [0013](https://github.com/Goldziher/monomyth/blob/main/adrs/0013-trait-first-planes-architecture.md) | Trait-first, planes-over-layers architecture |
| [0014](https://github.com/Goldziher/monomyth/blob/main/adrs/0014-contract-as-extraction-generation-pivot.md) | The contract as the extraction⇄generation pivot |
| [0015](https://github.com/Goldziher/monomyth/blob/main/adrs/0015-layered-override-configuration.md) | Layered override configuration (monomyth-config) |
| [0016](https://github.com/Goldziher/monomyth/blob/main/adrs/0016-build-time-methodology-and-law-synthesis.md) | Build-time methodology + reference-ingest law synthesis |
| [0017](https://github.com/Goldziher/monomyth/blob/main/adrs/0017-genre-as-config-dimension.md) | Genre as a cross-cutting config dimension |
| [0019](https://github.com/Goldziher/monomyth/blob/main/adrs/0019-user-uploaded-media-namespace.md) | User-uploaded media + user namespace/tier |
| [0021](https://github.com/Goldziher/monomyth/blob/main/adrs/0021-rust-architecture-guidelines-in-ai-rulez.md) | Rust architecture guidelines authored in ai-rulez |
| [0028](https://github.com/Goldziher/monomyth/blob/main/adrs/0028-genre-classifier-in-genre-crate.md) | GenreClassifier seam in monomyth-genre (contracts-edge exception) |

## Extraction & rendering

| ADR | Decision |
|---|---|
| [0018](https://github.com/Goldziher/monomyth/blob/main/adrs/0018-extraction-subsystem.md) | Extraction subsystem (monomyth-extract) |
| [0020](https://github.com/Goldziher/monomyth/blob/main/adrs/0020-medium-agnostic-rendering-adapters.md) | Medium-agnostic rendering adapters |

## Evaluation, synthesis & compose

| ADR | Decision |
|---|---|
| [0022](https://github.com/Goldziher/monomyth/blob/main/adrs/0022-scored-weighted-classification-attributes.md) | Scored / weighted classification attributes |
| [0023](https://github.com/Goldziher/monomyth/blob/main/adrs/0023-benchmark-driven-evaluation.md) | Benchmark-driven evaluation + ground-truth corpus |
| [0024](https://github.com/Goldziher/monomyth/blob/main/adrs/0024-llm-judge-feedback-loop-synthesis.md) | LLM-as-judge feedback loop for reference→law synthesis |
| [0025](https://github.com/Goldziher/monomyth/blob/main/adrs/0025-reference-retrieval-quality.md) | Reference retrieval quality: dedup, multi-query coverage, hybrid RRF |
| [0026](https://github.com/Goldziher/monomyth/blob/main/adrs/0026-reference-path-first-rag-enrichment.md) | Reference-path-first RAG enrichment (keywords + entities) |
| [0027](https://github.com/Goldziher/monomyth/blob/main/adrs/0027-long-form-compose-pipeline.md) | Long-form compose pipeline |

## Conventions

- One decision per file, named `NNNN-kebab-title.md` (zero-padded, monotonically increasing).
- `status` lifecycle: `proposed` → `accepted` → (later) `deprecated` / `superseded by ADR-NNNN`.
- All entries above are `accepted` unless noted otherwise inline.

The full index, kept in sync with the repository, lives at
[`adrs/README.md`](https://github.com/Goldziher/monomyth/blob/main/adrs/README.md).
