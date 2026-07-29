---
title: Crates
description: The 15 crates in the monomyth workspace, one line each.
---

monomyth is a Cargo workspace under `crates/`. The root is a virtual manifest.

| Crate | Purpose |
|---|---|
| `monomyth-core` | The domain model (World, Location, Entity, Item, Player, Story) and a pure, deterministic engine: `apply(world, action) -> Vec<Event>`. Depends on nothing. |
| `monomyth-frameworks` | The comparative-mythology domain schema as Rust enums + crosswalks over `artifacts/frameworks/*.json`, with parity tests. |
| `monomyth-gen` | Hybrid generation: deterministic procedural passes own structure; a non-deterministic content pass fills prose. |
| `monomyth-knowledge` | The ship-gated RAG layer: the license ledger, an in-tree vector-store base layer, and reference-path keyword/NER enrichment, over published xberg. |
| `monomyth-llm` | A thin, typed structured-output client (`generate::<T>()`) the content pass prompts, keeping the LLM provider out of the engine's public API. |
| `monomyth-contracts` | The cross-plane seam traits (`Classifier`, `Extractor`, `PassageRetriever`, `Renderer`) that keep extraction, generation, and rendering swappable. |
| `monomyth-config` | Layered TOML configuration: an override resolver over per-task model routing and generation/synthesis/compose knobs. An unconfigured run reproduces the defaults bit-for-bit. |
| `monomyth-genre` | Genre as a config-resolved dimension: a `GenreProfile` plus a `GenreClassifier` seam, so LitRPG, fantasy, sci-fi, or detective is a knob, not a fork. |
| `monomyth-synthesis` | The build-time, judge-gated pipeline that drafts abstract "law" taxonomies from reference-namespace priors for human review, never surfacing prose. |
| `monomyth-eval` | A deterministic, LLM-free benchmark-scoring harness plus an optional embedding-cosine semantic axis and performance baselines. |
| `monomyth-extract` | A hermetic, LLM-free RAG-softmax extraction proof-of-concept: text back into the structured model. |
| `monomyth-compose` | The long-form generation pipeline (Plan → Draft → Revise → Assemble) that turns a finished `World` into grounded adventure prose, quarantined from the procedural RNG. |
| `monomyth-render-terse` | A second, minimal `Renderer` implementation, proving the render seam supports more than one medium. |
| `monomyth-text` | The text frontend: pure string rendering over the model, the first `Renderer` implementation. |
| `monomyth-cli` | A thin binary wiring it together: `gen`, `play`, `edit`, `ingest`, `retrieve`, `corpus`, `compose`, `eval`, `finetune-export`, `synthesize`, and `extract`. |

Frontends and generation never depend on each other — the serialized world is the only thing that
crosses between them. See [Architecture](/monomyth/concepts/architecture/) for how these fit together
in planes, and [ADR-0001](https://github.com/Goldziher/monomyth/blob/main/adrs/0001-engine-plus-pluggable-frontends.md),
[ADR-0002](https://github.com/Goldziher/monomyth/blob/main/adrs/0002-hybrid-generation.md),
[ADR-0003](https://github.com/Goldziher/monomyth/blob/main/adrs/0003-slotmap-graph-domain-model.md).
