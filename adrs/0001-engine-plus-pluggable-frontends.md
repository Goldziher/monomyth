---
status: accepted
date: 2026-07-06
decision-makers: Na'aman Hirschfeld
---

# Engine + pluggable frontends

## Context and Problem Statement

monomyth must serve two presentations of the same adventures: a text-based interface now and
dynamically-generated pixel-art games later. How do we structure the system so both share one
source of truth without the graphics work forcing a rewrite of the text work (or vice versa)?

## Decision Drivers

- Two (eventually more) presentations over identical adventure content.
- The graphical frontend arrives much later; it must be a drop-in, not a fork.
- Testability and reproducibility of the core independent of any UI.

## Considered Options

- A core engine producing an abstract, serializable world/story model + separate frontend crates.
- A monolith with rendering woven through the domain logic.
- Text-first now, extract an engine later when graphics are needed.

## Decision Outcome

Chosen option: "core engine + pluggable frontends". A `monomyth-core` crate owns the domain model
and a pure `apply(world, action) -> Vec<Event>` state machine; frontends (`monomyth-text`,
later `monomyth-pixel`) consume the serialized model and semantic events. The model is
render-agnostic — no colors, glyphs, or coordinates leak in.

### Consequences

- Good, because the pixel-art frontend becomes additive: it depends on `monomyth-core` only.
- Good, because the core is testable and reproducible without any UI.
- Bad, because it demands upfront discipline: a clean, serializable contract before either UI exists.

### Confirmation

Frontends and generation depend on `monomyth-core` only, never on each other (enforced by the
Cargo dependency graph). The engine emits semantic `Event`s, not display strings.

## More Information

The serialized world is the only artifact that crosses between generation and frontends — the
property that makes the whole architecture composable. See ADR-0002, ADR-0003.
