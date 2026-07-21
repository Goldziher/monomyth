---
status: accepted
date: 2026-07-21
decision-makers: Na'aman Hirschfeld
---

# GenreClassifier seam lives in monomyth-genre, not monomyth-contracts

## Context and Problem Statement

ADR-0013's trait-first rule places every cross-plane seam trait in `monomyth-contracts` — the crate
that exists so plane boundaries can be programmed against without one impl crate depending on another.
ADR-0017 introduced the `GenreClassifier` seam (infer a `GenreProfile` from input text) and ADR-0018
declares its `Extractor` seam in `monomyth-contracts` as that rule prescribes. But `GenreClassifier`
returns a `GenreProfile`, a type **owned by `monomyth-genre`**. Where does the trait live without
either duplicating `GenreProfile` or forcing a dependency edge the plane model forbids?

## Decision Drivers

- ADR-0013's rule: seam traits live in `monomyth-contracts`, and impl crates depend only on those
  traits across a plane boundary, never on each other.
- A trait's signature drags its parameter and return types with it: declaring `GenreClassifier` in
  `monomyth-contracts` means `monomyth-contracts` must name `GenreProfile`.
- `GenreProfile` is a genre-plane (X-GENRE) value with real behavior (`ContentConfig` projection,
  layered-config resolution) — it belongs in `monomyth-genre`, not in the neutral contracts crate.
- `monomyth-contracts` is deliberately thin and low in the graph; adding a `monomyth-genre` dependency
  to it would pull the genre plane *under* the contracts crate every other plane depends on.

## Considered Options

- **Declare `GenreClassifier` in `monomyth-genre`** (beside `GenreProfile`), as a documented, narrow
  exception to ADR-0013's "seams live in contracts" rule.
- **Declare `GenreClassifier` in `monomyth-contracts`**, forcing a `monomyth-contracts → monomyth-genre`
  dependency edge.
- **Move `GenreProfile` into `monomyth-contracts`** so the trait and its return type share the crate.

## Decision Outcome

Chosen option: "declare `GenreClassifier` in `monomyth-genre`", because it keeps `GenreProfile` where
its behavior lives and keeps `monomyth-contracts` free of a genre dependency, at the cost of one
documented exception to the seam-location rule.

The exception is **scoped narrowly**: a seam trait may live in its own plane crate instead of
`monomyth-contracts` when its signature is dominated by a type that plane *owns* and that has no reason
to move into the neutral contracts crate. This is exactly the case for `GenreClassifier`/`GenreProfile`
and, prospectively, for the `Renderer` seam (ADR-0020) — which is instead kept genre-free precisely so
it *can* stay in `monomyth-contracts`. The rule of thumb: put the seam in `monomyth-contracts` by
default; relocate it to a plane crate only when doing so is the *only* way to avoid dragging a
plane-owned type down into contracts.

### Consequences

- Good, because `monomyth-contracts` stays a thin, genre-free hub — no plane sits beneath it that
  every other plane must then transitively pull in.
- Good, because `GenreProfile` stays in `monomyth-genre` with its `ContentConfig` projection and
  config-resolution behavior, rather than being split from the trait that returns it.
- Bad, because the "all seams live in `monomyth-contracts`" rule now has an exception a reader must
  know about; mitigated by this ADR and the rationale comment in `monomyth-genre/src/classifier.rs`.
- Bad, because a future consumer wanting to depend only on `monomyth-contracts` for *every* seam must
  also depend on `monomyth-genre` for this one; acceptable because any caller classifying genre already
  works with `GenreProfile` and so depends on `monomyth-genre` regardless.

### Confirmation

The rationale is documented at the trait's definition (`crates/monomyth-genre/src/classifier.rs`).
`crates/monomyth-core/tests/boundaries.rs` proves `monomyth-core` still has no path to `monomyth-genre`
or `monomyth-contracts`, so this exception does not weaken the pivot's purity (ADR-0013). Code review
confirms `monomyth-contracts` gains no `monomyth-genre` edge (`cargo tree -p monomyth-contracts`).

## Pros and Cons of the Options

### Declare `GenreClassifier` in `monomyth-genre`

- Good, because it avoids the forbidden `contracts → genre` edge while keeping `GenreProfile` where it
  belongs.
- Neutral, because it introduces a documented exception to ADR-0013's seam-location default.

### Declare `GenreClassifier` in `monomyth-contracts`

- Bad, because it forces `monomyth-contracts` to depend on `monomyth-genre`, inverting the graph so the
  genre plane sits beneath the neutral seam crate every plane depends on.

### Move `GenreProfile` into `monomyth-contracts`

- Bad, because `GenreProfile` carries genre-plane behavior (`ContentConfig` projection, layered-config
  resolution) that has no place in a neutral, behavior-free contracts crate, and it would drag
  `monomyth-config`/`monomyth-gen` concerns toward contracts.

## More Information

Refines ADR-0013 (trait-first seams) and ADR-0017 (which introduced `GenreClassifier`). The `Renderer`
seam (ADR-0020) applies the inverse lesson: it is deliberately kept genre-free so it *can* live in
`monomyth-contracts` — medium selection happens at the composition root instead of in the trait
signature. Revisit if `GenreProfile` is ever reduced to a contracts-safe plain-data type, at which
point the trait could move to `monomyth-contracts` without an edge.
