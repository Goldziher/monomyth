---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# Trait-first, planes-over-layers architecture

## Context and Problem Statement

monomyth is growing from a mythology-grounded text-adventure generator into a bidirectional,
medium-agnostic engine for narrative structure: it must *extract* structure from narrative text and
*generate* narrative/media from structure, for many media, driven by configuration synthesized from
source material. A first instinct is to model this as a numbered layer stack (sources → concepts →
rules → config → contract → transforms → genre → rendering). How should the system actually be
decomposed so the pieces stay swappable and the domain contract stays clean?

## Decision Drivers

- The serializable domain contract (`monomyth-core`) must stay a pure, dependency-free pivot — no IO,
  async, LLM, config, or genre concerns may leak into it.
- Backends and strategies (vector store, LLM, extractor, generation strategy, genre classifier,
  renderer) must be swappable and selected by configuration, not hard-wired.
- "Configuration" and "genre" are read by *every* transform; forcing them into a horizontal layer
  linearizes a dependency that does not exist and pollutes the contract.
- Build-time authoring (`map → synthesize → configure`) has a fundamentally different lifecycle from
  run-time transformation (extraction ⇄ generation) and should not be flattened into adjacent layers.

## Considered Options

- **Planes + cross-cutting concerns, trait-first seams:** six planes (Knowledge, Rules, Contract,
  Transform, Render) plus four cross-cutting concerns (Config, Genre, License, Determinism); every
  plane boundary is a trait; seam traits live in a dedicated `monomyth-contracts` crate.
- **An 8-level layer stack (L0–L7):** sources → concepts → rules → config → contract → transforms →
  genre → rendering as a linear dependency ladder.
- **A monolith with internal modules:** no crate-level seams; swap behavior by editing call sites.

## Decision Outcome

Chosen option: "Planes + cross-cutting concerns, trait-first seams", because it keeps the contract
pure, models config/genre as the cross-cutting projections they actually are, and makes every plane
boundary an injectable trait so impls swap by configuration.

The topology is **6 planes** — P-KNOW (`monomyth-knowledge`), P-RULE (`monomyth-frameworks` +
`artifacts/`), P-CONTRACT (`monomyth-core`, pure data), P-TRANSFORM (`monomyth-gen`,
`monomyth-extract`), P-RENDER (`monomyth-text` + peers) — plus **4 cross-cutting concerns**: X-CONFIG
(`monomyth-config`), X-GENRE (`monomyth-genre`), X-LICENSE (the ledger), X-DETERMINISM (core RNG + gen
seeding). P-KNOW+P-RULE are the build-time plane; P-TRANSFORM+P-RENDER the run-time plane.

**Trait-first rule:** every plane boundary is expressed as a trait, generalizing the existing
`Arc<dyn VectorStore>` / `dyn ProceduralPass` / `dyn ContentPass` / `dyn Embedder` seams. Seam traits
(which are frequently async/IO) live in a new **`monomyth-contracts`** crate — *not* in
`monomyth-core`, so the contract keeps its "no IO/async" invariant. Across a plane boundary, code
depends on `dyn Trait` / `impl Trait` from `monomyth-contracts`, never on a concrete impl crate; only
composition roots (`monomyth-cli`, `monomyth-text`) name concrete impls, and only to inject them.

### Consequences

- Good, because the contract stays a clean serializable pivot with no upward dependencies.
- Good, because config and genre reach transforms as values and trait objects, never as new fields on
  `World`, so the model stays medium- and genre-agnostic.
- Good, because a new backend/strategy/medium is one feature-gated trait impl plus a composition-root
  wire-up, with no call-site churn.
- Bad, because it adds crates (`monomyth-contracts`, `monomyth-config`, later `-genre`, `-extract`)
  and the discipline to route boundaries through traits; mitigated by trait-ifying *only* plane
  boundaries, never within a crate.

### Confirmation

A `cargo tree` CI check asserts `monomyth-core` has no inbound dependency on
`monomyth-config`/`monomyth-genre`/`monomyth-contracts` (the pivot depends on nothing). Code review
enforces that impl crates do not depend on each other across plane boundaries — only on
`monomyth-contracts` traits — with composition roots as the sole exception. The plane vocabulary and
the "core depends on nothing" rule are documented in `docs/architecture.md` and the `.ai-rulez`
guidelines (ADR-0021).

## Pros and Cons of the Options

### Planes + cross-cutting concerns, trait-first seams

- Good, because it matches the real dependency shape (config/genre cross-cut; the contract is a hub).
- Good, because it makes the build-time/run-time lifecycle split explicit (ADR-0016 builds on it).
- Neutral, because it front-loads some crate scaffolding before the payoff of swappable impls.

### 8-level layer stack (L0–L7)

- Bad, because it forces a false linearization ("rules become config become the contract") and tempts
  config/genre into the dependency path, polluting the pivot.
- Bad, because it hides the build-time vs run-time distinction that actually organizes the roadmap.

### Monolith with internal modules

- Good, because it is the least scaffolding up front.
- Bad, because behavior is not swappable by configuration and the contract's purity is unenforceable.

## More Information

Generalizes ADR-0001 (engine + pluggable frontends) from "text vs pixel" into a general trait-first
principle. Instantiated by ADR-0014 (the contract pivot), ADR-0015 (config), ADR-0017 (genre),
ADR-0018 (extraction), ADR-0020 (rendering). The Rust guideline that encodes "trait-first seams" for
all agents is ADR-0021. See `docs/architecture.md` for the authoritative plane map and crate graph.
