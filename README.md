<!-- markdownlint-disable MD033 MD041 -->
<div align="center">

<img src="docs/media/monomyth-banner.svg" alt="monomyth — one engine for every story" width="820">

**A deterministic engine for narrative structure — one story-world model, rendered to any medium.**

monomyth moves between unstructured narrative and a structured, medium-agnostic **world/story model**
in both directions: **generation** (model → text/media) today, **extraction** (text → model) next.
Structure is a pure function of a seed; a quarantined LLM pass fills the prose. Genre is a
configuration dimension and the medium is a swappable frontend — **text adventures across genres
today, a Sierra-style pixel-art frontend next.**

Deterministic · Corpus-grounded · Medium-agnostic · Generate ↔ extract · Genre as config

[![Docs](https://img.shields.io/badge/docs-goldziher.github.io%2Fmonomyth-e0b25e?style=flat-square)](https://goldziher.github.io/monomyth/)
[![CI](https://img.shields.io/github/actions/workflow/status/Goldziher/monomyth/ci.yml?branch=main&style=flat-square&color=e0b25e&label=CI)](https://github.com/Goldziher/monomyth/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-e0b25e?style=flat-square)](LICENSE)
[![Status: experimental](https://img.shields.io/badge/status-experimental-e0b25e?style=flat-square)](#roadmap)

[Docs](https://goldziher.github.io/monomyth/) · [What you get](#what-you-get) · [Roadmap](#roadmap) · [Origins](#origins) · [Architecture](#architecture)

</div>
<!-- markdownlint-enable MD033 MD041 -->

---

> **Experimental, pre-1.0.** The engine, its public API, and the serialized world/story schema will
> change between releases. Built in the open; not production-ready.

Its first instance, and its name, is Joseph Campbell's *monomyth* — the hero's journey — modeled
explicitly as the story spine and grounded in comparative-mythology scholarship. Everything is
deterministic: the same seed reproduces a byte-identical world, and the LLM content pass is
quarantined from the procedural RNG so it can only fill empty slots, never invent structure.

## What you get

monomyth is a headless engine — driven from a CLI harness or consumed as a library, over a single
serializable world/story contract. Answers are **structure**, not prose-until-you-ask-for-it.

| Capability | What it does | Key crate / command |
|---|---|---|
| **Deterministic world model** | A seed yields a byte-identical, serializable world, validated on load | `monomyth-core` · `gen --seed` |
| **Branching narrative spine** | A single-source, reconverging DAG of beats, mutated through a transactional edit vocabulary | `NarrativeStructure` · `edit` |
| **Hybrid generation** | Procedural passes own all structure; a quarantined LLM pass fills prose slots | `monomyth-gen` · `gen --fill` |
| **Long-form compose** | Plan → Draft → Revise → Assemble a `World` into grounded adventure prose | `monomyth-compose` · `compose` |
| **Corpus-grounded RAG** | Retrieval over a ship/reference-gated corpus; local ONNX embeddings | `monomyth-knowledge` · `ingest` / `retrieve` |
| **Build-time law synthesis** | Judge-gated distillation of abstract "laws" from reference priors, human-reviewed | `monomyth-synthesis` · `synthesize law` |
| **Extraction (proof-of-concept)** | Pull existing text back into the structured model | `monomyth-extract` · `extract` |
| **Deterministic evaluation** | Score extractors against ground-truth fixtures; export fine-tune pairs | `monomyth-eval` · `eval` |
| **Genre as configuration** | LitRPG, fantasy, sci-fi, detective as a resolved dimension, not a fork | `monomyth-genre` |
| **Pluggable frontends** | Render the same world to any medium — text now, pixel-art next | `monomyth-text` · `play` |

## Roadmap

monomyth is early and moves in the open. Detailed phases live in [`docs/roadmap.md`](./docs/roadmap.md).

**Now — in the engine.** The deterministic core and `apply(world, action)` state machine; the
branching narrative spine and its edit vocabulary; hybrid procedural + LLM generation; the
ship/reference RAG layer; layered TOML configuration; build-time law synthesis; the long-form compose
pipeline; benchmark-driven evaluation; an LLM-free extraction proof-of-concept; a text frontend; and
the CLI harness.

**Next.** Genre as a first-class config dimension (targeting → a `GenreClassifier`); extraction beyond
the single-node slice; full file-based configuration and per-task model routing; curated myth-theory
reference laws (Lévi-Strauss, Dumézil, Witzel, Doty) drafted and human-promoted; a synthesis
quality-baseline benchmark.

**Later.** User-uploaded corpora (a `user` namespace/tier, non-surfaceable by default); rendering
generalized behind a `Renderer` trait; a **Sierra-style pixel-art frontend** over the same world
model; further genres and mediums (LitRPG stat sheets, detective clue graphs).

## Architecture

A core engine produces an abstract, serializable **world/story model**; pluggable frontends render
it. The model is the shared contract and is strictly render-agnostic — no colors, glyphs, or screen
coordinates leak into it, which is exactly what lets a pixel-art frontend drop in later.

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
- **`monomyth-genre`** — genre as a config-resolved dimension: a `GenreProfile` plus a
  `GenreClassifier` seam, so LitRPG, fantasy, sci-fi, or detective is a knob, not a fork (ADR-0017).
- **`monomyth-synthesis`** — the build-time, judge-gated pipeline that drafts abstract "law"
  taxonomies from reference-namespace priors for human review, never surfacing prose (ADR-0016/0024).
- **`monomyth-eval`** — a deterministic, LLM-free benchmark-scoring harness plus an optional
  embedding-cosine semantic axis and performance baselines (ADR-0023).
- **`monomyth-extract`** — a hermetic, LLM-free RAG-softmax extraction proof-of-concept: text back
  into the structured model (ADR-0018).
- **`monomyth-compose`** — the long-form generation pipeline (Plan → Draft → Revise → Assemble) that
  turns a finished `World` into grounded adventure prose, quarantined from the procedural RNG (ADR-0027).
- **`monomyth-render-terse`** / **`monomyth-text`** — the render seam and the text frontend (pure
  string rendering over the model). **`monomyth-pixel`** comes later; frontends depend on
  `monomyth-core` only.
- **`monomyth-cli`** — a thin binary wiring it together: `gen`, `play`, `edit`, `ingest`,
  `retrieve`, `corpus`, `compose`, `eval`, `finetune-export`, `synthesize`, and `extract`.

Frontends and generation never depend on each other — the serialized world is the only thing that
crosses between them. See [ADR-0001](./adrs/0001-engine-plus-pluggable-frontends.md),
[ADR-0002](./adrs/0002-hybrid-generation.md), [ADR-0003](./adrs/0003-slotmap-graph-domain-model.md).

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

## Origins

monomyth grew out of an unfinished PhD on the *history of the study of myth* — not the myths
themselves, but how they have been read: the historiography of myth-scholarship, a chapter in the
history of knowledge. It builds on the thinkers I read then.

The story vocabulary is drawn from the comparative-mythology and narratological tradition — Campbell's
monomyth, Propp's morphology of the folktale, Polti's dramatic situations, Thompson's motif-index,
Greimas's actants. The roadmap reaches toward the deeper myth-theory of Lévi-Strauss, Dumézil,
Witzel, and Doty — distilled into abstract laws, never reproduced as prose (see
[licensing](#corpus-rag--licensing)).

From Wolfgang Iser and reader-response theory it takes a working directive: **a text constructs its
reader as deliberately as it projects an author's voice.** monomyth's generation is meant to build an
*implied reader* — the vantage from which a story becomes legible — not merely to imitate a narrator.

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

monomyth's output is permissively (MIT) licensed and may be used for any purpose, including
commercially — so corpus licensing is enforced, not aspirational. Every source is declared in
[`corpus/manifest.json`](./corpus/manifest.json) with `license` / `tier` / `namespace` / `domain`.
Two namespaces — `ship` (PD / CC0 / CC-BY / CC-BY-SA, may be surfaced verbatim) and `reference`
(copyrighted / NonCommercial, informs generation but is never redistributed) — with a hard invariant
enforced at ingest, retrieval, and CI. See
[ADR-0005](./adrs/0005-commercial-licensing-ship-reference.md).

## Repository layout

```text
crates/            Rust workspace: core, frameworks, contracts, config, genre, knowledge, llm, gen,
                   synthesis, eval, extract, compose, render-terse, text, cli
artifacts/
  frameworks/      the domain schema: validated framework + crosswalk JSON (committed)
corpus/            license & namespace ledger (manifest.json)
docs/              vision, architecture, roadmap, methodology
website/           the documentation site (Astro + Starlight) → GitHub Pages
adrs/              Architecture Decision Records (MADR)
.ai-rulez/         AI-assistant governance config (generates CLAUDE.md, etc. via ai-rulez)
poly.toml          poly (polylint) config — Rust + docs formatting/linting
```

## Development

- Task runner: `task --list` (see [`Taskfile.yaml`](./Taskfile.yaml)). `task check` is the full gate.
- Format & lint: `poly fmt --fix .` then `poly lint .` (`artifacts/**` is excluded — data, not source).
- Rust: `cargo fmt`, `cargo clippy --workspace --all-targets --tests -- -D warnings`,
  `cargo test --workspace`.
- Docs site: `cd website && npm ci && npm run dev`.

Conventions and agent guidance live in [`.ai-rulez/`](./.ai-rulez); the decision history lives in
[`adrs/`](./adrs).

## License

MIT — see [LICENSE](LICENSE).
