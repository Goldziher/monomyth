---
status: accepted
date: 2026-07-06
decision-makers: Na'aman Hirschfeld
---

# Local ONNX embeddings

## Context and Problem Statement

The RAG layer needs to embed documents and queries. xberg's `Embedder` seam supports local ONNX
models, provider-hosted (API) embeddings, and in-process plugins. Which do we use by default?

## Decision Drivers

- Self-contained, single-binary engine that runs offline (shippable desktop/game).
- No mandatory API keys or network round-trips at ingest/query time.
- Reasonable quality for retrieval over the corpus.

## Considered Options

- Local ONNX embeddings via xberg's `CoreEmbedder` (a preset model).
- Provider-hosted API embeddings (e.g. OpenAI text-embedding-3) via xberg's `Llm` embedding type.

## Decision Outcome

Chosen option: "local ONNX". Use `CoreEmbedder` with a local ONNX preset (feature
`pipeline-embeddings`). Embeddings run in-process with no network dependency. The `Embedder` trait
remains the swap point if we later want hosted embeddings for a server deployment.

### Consequences

- Good, because ingest and retrieval work fully offline, with no keys and no per-call cost.
- Good, because it fits the single-binary engine and reproducible builds.
- Bad, because local preset quality may trail the best hosted models; revisit if retrieval quality
  proves insufficient. The `Embedder` seam makes switching cheap.

### Confirmation

The knowledge crate embeds and retrieves with no network access in tests.

## More Information

Hosted embeddings pair naturally with a server/pgvector deployment (ADR-0008); local ONNX is the
default for the embedded engine. Note this is separate from the LLM used for content generation
(ADR-0002).
