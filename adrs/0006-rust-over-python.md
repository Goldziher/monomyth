---
status: accepted
date: 2026-07-06
decision-makers: Na'aman Hirschfeld
---

# Rust for the engine and corpus pipeline

## Context and Problem Statement

The engine is Rust (ADR-0001/0003). The corpus/RAG pipeline was prototyped in Python (stdlib fetch,
normalize, chunk). Do we keep a Python data layer alongside a Rust engine, or consolidate on Rust?

## Decision Drivers

- A self-contained, single-binary engine is a project goal (shippable desktop/game).
- The RAG engine we adopt (xberg, ADR-0007) is Rust; embeddings and chunking live there.
- One language reduces the ingest ↔ engine handoff, build/tooling surface, and drift.

## Considered Options

- Consolidate on Rust; the corpus pipeline becomes a Rust crate consuming xberg.
- Keep Python for data engineering, Rust for the engine, exchange JSONL artifacts.

## Decision Outcome

Chosen option: "consolidate on Rust". The corpus/RAG layer becomes the `monomyth-knowledge` crate
using xberg for document intelligence, chunking, and embeddings. The Python prototype is retired
once the Rust path lands. Language-neutral artifacts (`artifacts/frameworks/*.json`) stay as-is.

### Consequences

- Good, because the engine + retrieval ship as one binary with no Python runtime.
- Good, because xberg's Semantic chunker and embeddings replace the hand-rolled Python equivalents.
- Bad, because we re-implement the fetch/normalize/ledger logic in Rust — a one-time cost. The
  Python prototype's acquired corpus and design validated the approach and can seed the port.

### Confirmation

`monomyth-knowledge` ingests and retrieves without any Python dependency; the workspace builds and
tests as a single Cargo project.

## More Information

The Python prototype proved the pipeline (48 works, ~18k chunks, licensing invariant) and is kept in
history as the reference for the port. See ADR-0007, ADR-0008.
