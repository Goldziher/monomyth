---
priority: high
---

# monomyth — project overview

monomyth is an **adventure generation engine**: it produces text-based adventures now and,
later, dynamically-generated pixel-art games. Its thesis (and name) is Joseph Campbell's
monomyth — the hero's journey — modeled explicitly as the story spine.

## Architecture — engine + pluggable frontends

A core Rust engine produces an abstract, serializable **world/story model**; separate frontend
crates render it. The model is the shared contract and is strictly render-agnostic (no colors,
glyphs, or screen coordinates leak into it).

- `monomyth-core` — the domain model (World, Location, Entity, Item, Player, Story) + the engine
  (`apply(world, action) -> Vec<Event>`), a pure, deterministic state machine.
- `monomyth-gen` — hybrid generation: deterministic **procedural** passes own structure (maps,
  stats, pacing, quest skeletons); a non-deterministic **content** pass (LLM) fills prose/dialogue.
  The two phases are physically separated; procedural output is reproducible from a seed.
- `monomyth-knowledge` — the RAG layer over xberg (see corpus-and-rag context).
- `monomyth-text` (later) / `monomyth-pixel` (later) — frontends; depend on `monomyth-core` only.
- `monomyth-cli` — thin binary wiring it together.

Frontends and generation never depend on each other; the serialized world is the only thing that
crosses between them — that is what makes the pixel-art frontend a drop-in later.

## Hybrid generation

Procedural (seeded, deterministic) produces structure with empty content slots; the LLM stage only
transforms empty → filled and can never invent structure. Every content-bearing entity carries a
content slot alongside its structural fields, so either can be regenerated independently.

## Domain grounding

The story vocabulary is not invented — it is distilled from comparative-mythology scholarship into
`artifacts/frameworks/*.json`: a 3-tier plot model (Campbell macro-arc → Propp functions + Polti
situations → Thompson motifs) plus a character model (Propp role × Greimas actant × archetype),
bound by crosswalks. These JSON artifacts are language-neutral and feed the Rust `story` module.
