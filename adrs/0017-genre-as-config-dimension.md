---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# Genre as a cross-cutting config dimension (monomyth-genre)

## Context and Problem Statement

monomyth's target widens from mythology-grounded adventures to medium-agnostic narrative structure —
detective fiction, LitRPG, and other genres alongside myth. Genre visibly shapes output (pacing,
vocabulary, tone), so it is tempting to model it as a first-class type the domain model carries. But
ADR-0001 and ADR-0014 require `World`/`Story` to stay medium-agnostic and pure. Where does "genre"
live so it can shape generation and, later, classify input, without becoming a field on the contract?

## Decision Drivers

- The pure contract invariant (ADR-0014): `World`/`Story` may carry no genre-specific types — a
  detective story and a myth must serialize to the same shapes.
- Genre is read by every transform (generation content-fill, later extraction, later rendering), which
  is the signature of a cross-cutting concern, not a layer any one plane owns (ADR-0013).
- Two distinct roles exist and must not be conflated: *targeting* (bias generation toward a genre, an
  output concern, needed now) and *classification* (infer genre from input text, an input concern,
  needed only once extraction exists).
- Genre selection should compose with ADR-0015's layered config rather than invent a second
  configuration mechanism.

## Considered Options

- A new `monomyth-genre` crate holding a `GenreProfile` config value plus a `GenreClassifier` trait;
  wire targeting into generation now, defer the classifier's real implementation.
- Add a `genre: Genre` enum field directly to `World`/`Story`.
- Fold genre into `monomyth-gen` as an internal enum, with no dedicated crate or trait seam.

## Decision Outcome

Chosen option: "new `monomyth-genre` crate, `GenreProfile` + `GenreClassifier`". Genre has two roles,
kept distinct:

- **`GenreProfile`** — a config *value* (resolved through ADR-0015's `Layered<T>`/`ConfigResolver`,
  not a bespoke mechanism) that biases generation's content-fill and structural targeting toward a
  genre's conventions. This is the role implemented now.
- **`GenreClassifier`** — a trait (an ADR-0013 plane-boundary seam, living behind `dyn Trait`) that
  will infer a `GenreProfile` from input text once extraction (ADR-0018) exists. A stub/no-op impl is
  the only implementation for now; classification is explicitly deferred, not designed in depth here.

`monomyth-core` carries **no genre-specific type**: a `GenreProfile` reaches `monomyth-gen`'s
generation passes as a config value and a trait object, the same way `NarrativeConfig` already does —
never as a new field on `World`. A detective story and a myth produce structurally identical `World`/
`Story` shapes; only the content-fill prompts and structural targeting parameters they were generated
under differ, and those live outside the contract entirely.

### Consequences

- Good, because the contract stays medium- *and* genre-agnostic, preserving ADR-0014's guarantee as
  the product widens past mythology.
- Good, because targeting and classification are separated: the cheap, low-risk half (targeting) ships
  now, and the harder half (classification, which needs extraction to be meaningful) is deferred
  without blocking it.
- Good, because reusing ADR-0015's config mechanism means genre gets layered override (deployment vs.
  project vs. user genre defaults) for free, instead of a parallel resolution path.
- Bad, because a stub `GenreClassifier` is dead weight until extraction lands; accepted because the
  trait seam existing early means extraction (ADR-0018) can consume a stable classifier interface from
  its first slice rather than retrofitting one.

### Confirmation

A `cargo tree` check asserts `monomyth-core` has no inbound dependency on `monomyth-genre` (mirroring
ADR-0013's `-config`/`-contracts` check). A serialization-unchanged test generates worlds under two
different `GenreProfile` values and asserts both serialize to the same `World`/`Story` *shape*
(schema-identical; content differs). Generated content under a non-myth profile (e.g. detective)
measurably differs from the myth default, confirming targeting actually reaches content-fill.

## Pros and Cons of the Options

### `monomyth-genre` crate: `GenreProfile` config + `GenreClassifier` trait

- Good, because it matches the real shape of the concern (cross-cutting, config-resolved,
  trait-seamed) instead of forcing genre into either the contract or a single consuming crate.
- Good, because splitting targeting from classification lets the roadmap sequence them independently
  (Phase 2 targeting, Phase 3+ classification) without redesigning the crate later.
- Neutral, because it is a new crate for a currently-thin feature set (one config type, one stub
  trait) — justified by the plane-boundary discipline ADR-0013 already commits to.

### `genre` field directly on `World`/`Story`

- Bad, because it violates ADR-0014's pure-contract invariant outright — genre becomes something every
  consumer of `World` must know about, including ones (a save/load viewer, a future renderer) that
  have no reason to.

### Genre as an internal `monomyth-gen` enum, no dedicated crate

- Bad, because it is invisible to any future consumer outside `monomyth-gen` — a renderer or extractor
  wanting genre-awareness would have to depend on the generation crate itself, crossing a plane
  boundary ADR-0013 explicitly forbids.

## More Information

Instantiates ADR-0013's X-GENRE cross-cutting concern. Consumes ADR-0015 (the config resolver genre
values are layered through). Constrained by ADR-0014 (the contract-purity invariant this ADR is the
concrete guard for). Scope note: this ADR covers genre's representation and its two transform roles —
the `GenreClassifier`'s internal ML/heuristic design is out of scope, deferred to when extraction
(ADR-0018) makes classification meaningful.
