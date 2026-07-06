---
status: accepted
date: 2026-07-06
decision-makers: Na'aman Hirschfeld
---

# Adopt xberg as the RAG engine

## Context and Problem Statement

monomyth needs document intelligence (parsing many formats), chunking, embeddings, a vector store,
and retrieval. Building this from scratch in Rust is substantial and off the project's critical path
(the adventure engine). What do we use for the RAG layer?

## Decision Drivers

- Rust-native (ADR-0006), single-binary friendly.
- A backend-agnostic vector-store abstraction so we can start embedded and scale later.
- Document intelligence + chunking + embeddings + retrieval without assembling many crates ourselves.

## Considered Options

- Adopt `xberg-rag` (+ `xberg` core) and implement/inject a `VectorStore`.
- Assemble our own stack (e.g. a vector crate + a chunker + an embedding crate + glue).

## Decision Outcome

Chosen option: "adopt xberg". Consume `xberg-rag` (crates.io, `1.0.0-rc.*`) with features
`vector-store` + `sqlite` + `pipeline` + `pipeline-embeddings`, plus `xberg` core for extraction /
chunking / embeddings. Its `VectorStore` is a 9-method async trait over neutral serde types; the
free-function `pipeline` drives chunk → embed → upsert → query. We inject `Arc<dyn VectorStore>` and
do **not** fork xberg.

### Consequences

- Good, because we get document intelligence, a Semantic chunker, embeddings, hybrid search, and a
  clean backend seam essentially for free.
- Good, because the trait is the intended external extension point (backends are just impls).
- Bad, because we depend on a pre-1.0 (`rc.*`) crate — pin the version and track releases.

### Confirmation

`monomyth-knowledge` compiles against `xberg-rag`, ingests a document, and retrieves it via the
built-in store — a smoke test in the crate.

## More Information

xberg ships in-memory and sqlite-vec backends; LanceDB/pgvector are the external adapter seam. See
ADR-0008 (backend choice) and ADR-0009 (embeddings).
