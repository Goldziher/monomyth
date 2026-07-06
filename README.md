# monomyth

An **adventure generation engine** — text-based adventures now, dynamically-generated pixel-art
games later — grounded in comparative mythology. The name is Joseph Campbell's *monomyth*, the
hero's journey, which the engine models explicitly as the story spine.

> Status: early. The comparative-mythology **domain schema** and a data-acquisition prototype exist;
> the Rust engine and the xberg-backed RAG layer are being built. See [`adrs/`](./adrs) for the
> architecture decisions and their rationale.

## Architecture

A core engine produces an abstract, serializable **world/story model**; pluggable frontends render
it. The model is the shared contract and is strictly render-agnostic.

- **`monomyth-core`** — the domain model (World, Location, Entity, Item, Player, Story) and a pure,
  deterministic engine: `apply(world, action) -> Vec<Event>`.
- **`monomyth-gen`** — hybrid generation: deterministic **procedural** passes own structure (maps,
  stats, pacing, quest skeletons); a non-deterministic **content** pass (LLM) fills prose. Structure
  is reproducible from a seed; the LLM only fills content slots and can never invent structure.
- **`monomyth-knowledge`** — the RAG layer, consuming [xberg](https://crates.io/crates/xberg-rag)
  for document intelligence, chunking, embeddings, a vector store, and retrieval.
- **`monomyth-text`** / **`monomyth-pixel`** (later) — frontends; depend on `monomyth-core` only.

Frontends and generation never depend on each other — the serialized world is the only thing that
crosses between them, which is what makes the pixel-art frontend a drop-in later. See
[ADR-0001](./adrs/0001-engine-plus-pluggable-frontends.md), [ADR-0002](./adrs/0002-hybrid-generation.md),
[ADR-0003](./adrs/0003-slotmap-graph-domain-model.md).

## Domain schema — grounded, not invented

The story vocabulary is distilled from comparative-mythology scholarship into validated,
language-neutral JSON under [`artifacts/frameworks/`](./artifacts/frameworks): a **3-tier plot
model** (Campbell monomyth → Propp functions + Polti situations → Thompson motifs) plus a
**character model** (Propp role × Greimas actant × archetype), bound by crosswalks. The Rust `story`
module reads these directly. See [ADR-0004](./adrs/0004-mythology-frameworks-as-domain-schema.md).

## Corpus, RAG & licensing

monomyth grounds generation in a corpus of mythology, folklore, esoterica, and SF/F, retrieved via
xberg ([ADR-0007](./adrs/0007-adopt-xberg-rag-engine.md)). The embedded store is xberg's sqlite-vec
backend today, with LanceDB/pgvector adapters as the external seam later
([ADR-0008](./adrs/0008-vectorstore-backend-sqlite-then-lancedb.md)); embeddings run locally via ONNX
([ADR-0009](./adrs/0009-local-onnx-embeddings.md)).

This is a **commercial** product, so licensing is enforced, not aspirational. Every source is
declared in [`corpus/manifest.json`](./corpus/manifest.json) with `license` / `tier` / `namespace` /
`domain`. Two namespaces — `ship` (PD / CC0 / CC-BY / CC-BY-SA, may be surfaced verbatim) and
`reference` (copyrighted / NonCommercial, informs generation but is never redistributed) — with a
hard invariant enforced at ingest, retrieval, and CI. See
[ADR-0005](./adrs/0005-commercial-licensing-ship-reference.md).

## Repository layout

```text
crates/            Rust workspace (engine, generation, knowledge, frontends, cli) — being built
artifacts/
  frameworks/      the domain schema: validated framework + crosswalk JSON (committed)
corpus/            license & namespace ledger (manifest.json)
adrs/              Architecture Decision Records (MADR)
.ai-rulez/         AI-assistant governance config (generates CLAUDE.md, etc. via ai-rulez)
poly.toml          poly (polylint) config — Rust + docs formatting/linting
```

## Development

- Format & lint: `poly fmt --fix .` then `poly lint .` (`artifacts/**` is excluded — data, not source).
- Rust: `cargo fmt`, `cargo clippy --workspace -- -D warnings`, `cargo test --workspace`.

Ingestion, retrieval, and validation of the framework artifacts are owned by the Rust
`monomyth-knowledge` crate (xberg) — being built.

Conventions and agent guidance live in [`.ai-rulez/`](./.ai-rulez); the decision history lives in
[`adrs/`](./adrs).
