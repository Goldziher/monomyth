# monomyth

monomyth is a **configurable, layered engine for narrative structure**. It moves between
unstructured narrative and a structured, medium-agnostic **world/story model** in *both directions* —
**generation** (model → text/media) today, **extraction** (text → model) next — for any narrative
medium (prose, games, LitRPG, detective fiction, apps), grounded in source material. Its first
instance, and its name, is Joseph Campbell's *monomyth* — the hero's journey — modeled explicitly as
the story spine.

> Status: pre-1.0, under active development. **Built today:** the domain model and deterministic
> engine, hybrid (procedural + LLM) generation, the xberg-backed RAG layer with reference-path
> keyword/NER enrichment, layered TOML configuration, build-time law synthesis, a benchmark-driven
> evaluation harness, a long-form compose pipeline, an LLM-free extraction proof-of-concept, a text
> frontend, and a CLI. **Forthcoming** (see the roadmap): genre as a config dimension, user-uploaded
> corpora, and additional render adapters (a pixel-art frontend). It is an **engine**: a headless,
> deterministic, corpus-grounded system that *produces and transforms* a serializable world/story
> model. The CLI is a harness, not the product.
>
> **Start here:** [`docs/vision.md`](./docs/vision.md) · [`docs/architecture.md`](./docs/architecture.md)
> · [`docs/roadmap.md`](./docs/roadmap.md). Decisions live in [`adrs/`](./adrs).

## Architecture

A core engine produces an abstract, serializable **world/story model**; pluggable frontends render
it. The model is the shared contract and is strictly render-agnostic.

- **`monomyth-core`** — the domain model (World, Location, Entity, Item, Player, Story) and a pure,
  deterministic engine: `apply(world, action) -> Vec<Event>`. Includes `World::validate` /
  `World::from_json_checked`, the enforced integrity gate for the serialized contract.
- **`monomyth-frameworks`** — the comparative-mythology domain schema as Rust enums + crosswalks over
  the `artifacts/frameworks/*.json` artifacts, with parity tests.
- **`monomyth-gen`** — hybrid generation: deterministic **procedural** passes own structure (the
  branching narrative spine, maps, cast, items); a non-deterministic **content** pass (LLM) fills
  prose. Structure is reproducible from a seed; the LLM only fills content slots and can never invent
  structure.
- **`monomyth-knowledge`** — the ship-gated RAG layer: the license ledger, an in-tree vector-store
  base layer, and reference-path keyword/NER enrichment, over published
  [xberg](https://crates.io/crates/xberg) for document intelligence, chunking, and embeddings.
- **`monomyth-llm`** — a thin, typed structured-output client (`generate::<T>()`) the content pass
  prompts, keeping the LLM provider out of the engine's public API.
- **`monomyth-contracts`** — the cross-plane seam traits (`Classifier`, `Extractor`,
  `PassageRetriever`) that keep extraction, generation, and retrieval swappable (ADR-0013).
- **`monomyth-config`** — layered TOML configuration: an override resolver over per-task model
  routing and generation/synthesis/compose knobs; an unconfigured run reproduces the defaults
  bit-for-bit (ADR-0015).
- **`monomyth-synthesis`** — the build-time, judge-gated pipeline that drafts abstract "law"
  taxonomies from reference-namespace priors for human review, never surfacing prose (ADR-0016/0024).
- **`monomyth-eval`** — a deterministic, LLM-free benchmark-scoring harness plus an optional
  embedding-cosine semantic axis and performance baselines (ADR-0023).
- **`monomyth-extract`** — a hermetic, LLM-free RAG-softmax extraction proof-of-concept: text back
  into the structured model (ADR-0018).
- **`monomyth-compose`** — the long-form generation pipeline (Plan → Draft → Revise → Assemble) that
  turns a finished `World` into grounded adventure prose, quarantined from the procedural RNG (ADR-0027).
- **`monomyth-text`** — the text frontend (pure string rendering over the model). **`monomyth-pixel`**
  comes later; frontends depend on `monomyth-core` only.
- **`monomyth-cli`** — a thin binary wiring it together: `gen`, `play`, `edit`, `ingest`,
  `retrieve`, `corpus`, `compose`, `eval`, `finetune-export`, and `synthesize`.

Frontends and generation never depend on each other — the serialized world is the only thing that
crosses between them, which is what makes the pixel-art frontend a drop-in later. See
[ADR-0001](./adrs/0001-engine-plus-pluggable-frontends.md), [ADR-0002](./adrs/0002-hybrid-generation.md),
[ADR-0003](./adrs/0003-slotmap-graph-domain-model.md).

### The story spine is a branching graph

The narrative is a first-class, serializable **`NarrativeStructure`**: a single-source, acyclic,
**reconverging DAG** of beats, not a flat list. Player choices fork the trunk and reconverge, so it
is a real branching structure — grounded, not invented: fork diamonds open at the corpus-flagged
*optional* Campbell stages. It is mutated only through a serializable **edit vocabulary**
(`NarrativeEdit`, applied transactionally with a validate-or-roll-back contract), so generation, human
edits (`monomyth edit`), and future LLM agents all drive the *same* surface. Traversal is a pure
engine action (`Action::Choose`) advancing a cursor along available, optionally guarded edges. See
[ADR-0003](./adrs/0003-slotmap-graph-domain-model.md).

## Usage

```sh
# Generate a world from a seed and render its branching narrative structure (deterministic).
cargo run -p monomyth-cli -- gen --seed 42

# Write the serialized world to a file, then walk its forks interactively.
cargo run -p monomyth-cli -- gen --seed 42 --out world.json
cargo run -p monomyth-cli -- play --world world.json

# Apply a JSON script of narrative edit operations to a world, re-validated transactionally.
cargo run -p monomyth-cli -- edit --world world.json --script edits.json

# Also fill prose slots from the ship-gated corpus via the LLM (requires provider config).
cargo run -p monomyth-cli -- gen --seed 42 --fill

# Compose long-form adventure prose for a seed (Plan → Draft → Revise → Assemble; requires provider config).
cargo run -p monomyth-cli -- compose --seed 42 --out adventure.md

# Score an extractor against a ground-truth benchmark fixture, or export PD-gated fine-tune pairs.
cargo run -p monomyth-cli -- eval --work odyssey_campbell_macro
cargo run -p monomyth-cli -- finetune-export --work odyssey_campbell_macro --out pairs.jsonl

# Draft a pre-review candidate "law" (an abstract taxonomy) from reference-namespace priors.
cargo run -p monomyth-cli -- synthesize law --law three_act --domain myth --query "story structure"

# Build, or audit against the license ledger, the ship-safe corpus declared in the manifest.
cargo run -p monomyth-cli -- corpus build
cargo run -p monomyth-cli -- corpus audit
```

The same `seed` always reproduces a byte-identical world; the LLM content pass is quarantined from the
procedural RNG and only fills empty slots.

## Domain schema — grounded, not invented

The story vocabulary is distilled from comparative-mythology scholarship into validated,
language-neutral JSON under [`artifacts/frameworks/`](./artifacts/frameworks): a **3-tier plot
model** (Campbell monomyth → Propp functions + Polti situations → Thompson motifs) plus a
**character model** (Propp role × Greimas actant × archetype), bound by crosswalks. The
`monomyth-frameworks` crate reads these directly. See
[ADR-0004](./adrs/0004-mythology-frameworks-as-domain-schema.md).

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
crates/            Rust workspace: core, frameworks, contracts, config, knowledge, llm, gen,
                   synthesis, eval, extract, compose, text, cli
artifacts/
  frameworks/      the domain schema: validated framework + crosswalk JSON (committed)
corpus/            license & namespace ledger (manifest.json)
adrs/              Architecture Decision Records (MADR)
.ai-rulez/         AI-assistant governance config (generates CLAUDE.md, etc. via ai-rulez)
poly.toml          poly (polylint) config — Rust + docs formatting/linting
```

## Development

- Format & lint: `poly fmt --fix .` then `poly lint .` (`artifacts/**` is excluded — data, not source).
- Rust: `cargo fmt`, `cargo clippy --workspace --all-targets --tests -- -D warnings`,
  `cargo test --workspace`.

Conventions and agent guidance live in [`.ai-rulez/`](./.ai-rulez); the decision history lives in
[`adrs/`](./adrs).
