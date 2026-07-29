---
title: Genres
description: Genre as a config dimension — GenreProfile and the GenreClassifier seam — so LitRPG, fantasy, sci-fi, and detective fiction are knobs, not forks.
---

monomyth's target widens from mythology-grounded adventures to medium-agnostic narrative structure —
detective fiction, LitRPG, and other genres alongside myth. Genre visibly shapes output (pacing,
vocabulary, tone), which makes it tempting to model as a type the domain model carries. It isn't:
`World`/`Story` must stay medium- and genre-agnostic, so a detective story and a myth serialize to the
same shapes.

## Two distinct roles

Genre has two roles, and they are kept separate:

- **Targeting** — biasing generation's content-fill and structural targeting toward a genre's
  conventions. This is an output concern, and it is implemented today.
- **Classification** — inferring a genre from input text. This is an input concern, meaningful only
  once extraction exists to produce text to classify. It is deferred.

## `GenreProfile` and `GenreClassifier`

`monomyth-genre` ships both halves as distinct types:

- **`GenreProfile`** — a config *value*, resolved through the same layered override mechanism as
  every other tunable (`monomyth-config`'s `ConfigResolver`), not a bespoke mechanism. It reaches
  `monomyth-gen`'s content-fill as a `ContentConfig` projection, the same way `NarrativeConfig`
  already does — never as a new field on `World`.
- **`GenreClassifier`** — a trait seam that will infer a `GenreProfile` from input text once
  extraction exists. Today it has a single stub/no-op implementation
  (`StubGenreClassifier`); classification's real design is explicitly out of scope until extraction
  needs it.

`monomyth-core` carries no genre-specific type at all. A detective story and a myth produce
structurally identical `World`/`Story` shapes; only the content-fill prompts and structural targeting
parameters they were generated under differ, and those live entirely outside the contract. This is
machine-enforced, not just a convention — a serialization-unchanged test plus a `cargo tree` boundary
check in `monomyth-core`'s test suite assert the crate has no inbound `-genre` edge.

## What's a knob today

Adding a genre is priors plus config, not a rewrite. The genre tests assert a non-myth profile
(detective) yields different `ContentConfig` instructions than the myth default, while the procedural
determinism golden stays byte-identical — config biases content prompts, never the RNG.

## Roadmap

Genre targeting is landed (`monomyth-genre`'s `GenreProfile`, wired into `monomyth-gen` and the CLI).
Genre *classification* is deferred until the [extraction](/monomyth/reference/crates/) subsystem
produces text to classify — see the [roadmap](https://github.com/Goldziher/monomyth/blob/main/docs/roadmap.md).

See [ADR-0017](https://github.com/Goldziher/monomyth/blob/main/adrs/0017-genre-as-config-dimension.md)
and [ADR-0028](https://github.com/Goldziher/monomyth/blob/main/adrs/0028-genre-classifier-in-genre-crate.md).
