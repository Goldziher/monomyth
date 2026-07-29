---
title: Architecture
description: The engine + pluggable frontends split, the world/story model as the shared contract, and the crates that implement it.
---

A core engine produces an abstract, serializable **world/story model**; pluggable frontends render
it. The model is the shared contract and is strictly render-agnostic — no colors, glyphs, or screen
coordinates leak into it, which is exactly what lets a pixel-art frontend drop in later.

## The contract

`monomyth-core` holds the domain model — `World`, `Location`, `Entity`, `Item`, `Player`, `Story` —
and a pure, deterministic engine: `apply(world, action) -> Vec<Event>`. `World::validate` and
`World::from_json_checked` are the enforced integrity gate for the serialized contract: nothing
outside `monomyth-core` mutates the model directly, and anything crossing the load boundary is
checked before it is trusted.

The contract depends on nothing — no IO, no async, no LLM/RAG concerns, no config or genre types.
Every other crate reads or writes it; it never reads back.

## Planes, not a layer stack

The rest of the system is decomposed into six planes plus four cross-cutting concerns, not a linear
layer stack. A layer stack would force config and genre into the dependency path and pollute the
contract; planes model the real shape instead — the contract is a hub, and config/genre are
projections every transform reads, not layers between other layers.

| Plane / concern | Crate(s) | Lifecycle |
|---|---|---|
| Knowledge (sources + RAG) | `monomyth-knowledge` | build + run |
| Rules / Frameworks / Laws | `monomyth-frameworks` + `artifacts/` | build-time |
| Narrative contract (the pivot) | `monomyth-core` | run-time, pure data |
| Extraction + Generation | `monomyth-gen`, `monomyth-extract` | run-time |
| Rendering adapters | `monomyth-text`, `monomyth-render-terse` | run-time |
| Config (cross-cutting) | `monomyth-config` | cross-cutting |
| Genre (cross-cutting) | `monomyth-genre` | cross-cutting |
| Licensing (cross-cutting) | `monomyth-knowledge` ledger | cross-cutting |

Knowledge and Rules together form the **build-time plane**: occasional, human-reviewed work that
produces vocabulary. Extraction/Generation and Rendering form the **run-time plane**: automated
transforms that execute per request, deterministically where structure is concerned.

## Crate roles

- **`monomyth-core`** — the domain model and the deterministic engine. The pure pivot everything else
  targets.
- **`monomyth-frameworks`** — the comparative-mythology domain schema as Rust enums + crosswalks over
  the `artifacts/frameworks/*.json` artifacts, with parity tests.
- **`monomyth-gen`** — hybrid generation: deterministic procedural passes own structure; a
  non-deterministic content pass fills prose. See [Hybrid generation](/monomyth/concepts/hybrid-generation/).
- **`monomyth-knowledge`** — the ship-gated RAG layer: the license ledger, an in-tree vector-store
  base layer, and reference-path keyword/NER enrichment, over published
  [xberg](https://crates.io/crates/xberg).
- **`monomyth-llm`** — a thin, typed structured-output client (`generate::<T>()`) the content pass
  prompts, keeping the LLM provider out of the engine's public API.
- **`monomyth-contracts`** — the cross-plane seam traits (`Classifier`, `Extractor`,
  `PassageRetriever`, `Renderer`) that keep extraction, generation, and rendering swappable.
- **`monomyth-config`** — layered TOML configuration: an override resolver over per-task model
  routing and generation/synthesis/compose knobs. See [Configuration](/monomyth/reference/configuration/).
- **`monomyth-genre`** — genre as a config-resolved dimension. See [Genres](/monomyth/concepts/genres/).
- **`monomyth-synthesis`** — the build-time, judge-gated pipeline that drafts abstract "law"
  taxonomies from reference-namespace priors for human review, never surfacing prose.
- **`monomyth-eval`** — a deterministic, LLM-free benchmark-scoring harness plus an optional
  embedding-cosine semantic axis and performance baselines.
- **`monomyth-extract`** — the extraction proof-of-concept: text back into the structured model.
- **`monomyth-compose`** — the long-form generation pipeline (Plan → Draft → Revise → Assemble) that
  turns a finished `World` into grounded adventure prose, quarantined from the procedural RNG.
- **`monomyth-render-terse`** / **`monomyth-text`** — the render seam and the text frontend. See
  [Frontends](/monomyth/concepts/frontends/).
- **`monomyth-cli`** — a thin binary wiring it together.

The full one-line-each table lives in the [crate reference](/monomyth/reference/crates/).

## Boundary rules

Frontends and generation never depend on each other — the serialized world is the only thing that
crosses between them. Across a plane boundary, code depends on `dyn Trait` / `impl Trait` from
`monomyth-contracts`, never on a concrete implementation crate. Only composition roots
(`monomyth-cli`, `monomyth-text`) name concrete impl crates, and only to inject them.

See [ADR-0001](https://github.com/Goldziher/monomyth/blob/main/adrs/0001-engine-plus-pluggable-frontends.md),
[ADR-0013](https://github.com/Goldziher/monomyth/blob/main/adrs/0013-trait-first-planes-architecture.md),
and the [decisions index](/monomyth/decisions/) for the full reasoning.
