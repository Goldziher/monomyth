---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# Medium-agnostic rendering adapters (Renderer trait)

## Context and Problem Statement

ADR-0001 committed to an engine + pluggable frontends split so a pixel-art frontend could later be a
drop-in beside `monomyth-text`. The roadmap now widens "frontend" from a binary text-vs-pixel choice
to an open set of media — prose, structured game text, LitRPG, detective fiction — selected by genre
targeting (ADR-0017), not hard-coded. How do we generalize the frontend seam so any renderer is a
peer implementation over the same contract, without any medium concern reaching back into `World`?

## Decision Drivers

- `monomyth-core`'s `World`/`Story` must stay render-agnostic (ADR-0001's existing constraint) — no
  colors, glyphs, screen coordinates, or prose-specific fields may leak into the pivot.
- The set of target media is open-ended and genre-selected; hard-wiring "text or pixel" in the type
  system does not scale to N media chosen by config.
- Every plane boundary in the architecture is a trait with swappable, config-selected implementations
  (ADR-0013's trait-first rule); rendering is the P-RENDER plane and should follow the same shape as
  P-TRANSFORM's `Extractor`/`GenerationStrategy` seams.
- `monomyth-text` already exists as a working frontend; generalizing must not force a rewrite, only a
  reframing behind a trait.

## Considered Options

- A thin `Renderer` trait (P-RENDER plane, declared in `monomyth-contracts`), one implementation per
  medium, consuming only the pure contract plus config/genre; `monomyth-text` becomes its first impl.
- Keep the current ad hoc `monomyth-text` shape and add new media as separate, unrelated crates with
  no shared trait.
- Push medium-specific hints (e.g. "this beat renders as combat" ) onto `World`/`Story` fields so
  renderers can branch on them directly.

## Decision Outcome

Chosen option: "A thin `Renderer` trait, one impl per medium", because it generalizes the existing
ADR-0001 frontend split into the general trait-first shape every other plane boundary already uses,
without touching the contract.

`Renderer` is declared in `monomyth-contracts` (ADR-0013) and takes a `&World` (or `&Story`) plus
resolved config/genre context, producing medium-specific output; it is the run-time counterpart to
extraction (ADR-0018) in the P-RENDER plane. Each medium — prose, a structured game format, LitRPG,
detective fiction — is its own crate implementing `Renderer`, selected at the composition root
(`monomyth-cli` / a future launcher) by config, never by a hard-coded branch. `monomyth-text` is
refactored to be the first `Renderer` impl rather than a bespoke frontend; no behavior change is
required beyond sitting behind the trait. **The constraint that makes this safe:** a `Renderer` impl
consumes only `monomyth-core`'s public API plus config/genre values passed in as parameters — it
never gains its own field on `World`, and no renderer-specific type crosses back into the contract.
Rendering stays a leaf: nothing downstream of `Renderer` feeds back into P-CONTRACT or P-TRANSFORM.

### Consequences

- Good, because a new medium is one feature crate implementing one trait plus a composition-root
  wire-up — no call-site churn in `monomyth-core` or `monomyth-gen`/`monomyth-extract`.
- Good, because the contract stays provably medium-agnostic: renderers cannot special-case a medium by
  adding fields, because they only ever read the existing public surface.
- Good, because `monomyth-text` requires no rewrite, only a thin trait-conformance pass.
- Bad, because some medium-specific needs (e.g. a game format wanting stat blocks the prose renderer
  ignores) must be expressible from the *existing* contract rather than a bespoke field; mitigated by
  routing genuinely new structural needs through ADR-0014's `SCHEMA_VERSION` gate, same as extraction.

### Confirmation

Two `Renderer` implementations (`monomyth-text` and a second, minimal medium) run over the same
serialized `World`, selected purely by config — no code path branches on medium outside the
composition root. A `cargo tree` / code-review check confirms renderer crates depend only on
`monomyth-core`'s public API (plus `monomyth-contracts`/`monomyth-config`/`monomyth-genre` as needed)
and never on each other.

## Pros and Cons of the Options

### Thin `Renderer` trait, one impl per medium

- Good, because it matches the trait-first shape of every other plane boundary (ADR-0013), so the
  mental model for "how do I add an X" is the same across extraction, generation, and rendering.
- Good, because it makes the "rendering never leaks upward" constraint a checkable property (renderer
  crates' dependency graph), not just a convention.
- Neutral, because it requires touching `monomyth-text` to sit behind the trait, though as a thin
  wrapper rather than a rewrite.

### Ad hoc frontend crates, no shared trait

- Bad, because it reintroduces exactly what ADR-0001 avoided: no common seam means the composition
  root cannot select a medium by configuration, and nothing prevents a new frontend from reaching
  around the contract into implementation details of another frontend.

### Push medium hints onto `World`/`Story`

- Bad, because it directly violates the render-agnostic pivot ADR-0001 established: `World` would
  accumulate fields meaningful to one renderer and useless (or actively confusing) to every other,
  and genre/config would leak into the contract exactly as ADR-0013 warns against.

## More Information

Generalizes ADR-0001 (engine + pluggable frontends) from "text vs pixel" to an open set of media.
Consumes ADR-0017 (genre as a cross-cutting config dimension) for medium selection/targeting. Run-time
counterpart to ADR-0018 (extraction): together they are the two P-TRANSFORM/P-RENDER faces of the
contract in the roadmap's plane model (ADR-0013). Implementation is Phase 5 of the roadmap
(`docs/roadmap.md`), after the config spine, genre, and extraction are stable.
