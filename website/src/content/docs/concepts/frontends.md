---
title: Frontends
description: Medium-agnostic model, swappable renderers — the text frontend today, a Sierra-style pixel-art frontend next.
---

A core engine produces the world/story model; pluggable frontends render it. The model carries no
presentation — no colors, glyphs, or screen coordinates — so a renderer is a thin, downstream,
contract-only adapter.

## The `Renderer` seam

Rendering is its own plane, with an open set of target media selected by config rather than a
hard-coded text-vs-pixel branch. A `Renderer` trait, declared in `monomyth-contracts`, takes a
`&World` (or `&Story`) plus resolved config/genre context and produces medium-specific output. Each
medium — prose, a structured game format, a future pixel-art scene graph — is its own crate
implementing `Renderer`, selected at a composition root (`monomyth-cli`) by a `[render].medium`
config value. `gen`/`play`/`edit` hold only a `Box<dyn Renderer>`; nothing branches on medium outside
that one wiring point.

The constraint that makes this safe: a `Renderer` implementation consumes only `monomyth-core`'s
public API plus config/genre values passed in as parameters. It never gains its own field on `World`,
and no renderer-specific type crosses back into the contract. Rendering is a leaf — nothing
downstream of it feeds back into the contract or the transform passes.

## Today: text

`monomyth-text` and `monomyth-render-terse` are the shipped `Renderer` implementations — pure string
rendering over the model. `monomyth-text`'s `TextRenderer` is a thin wrapper over the existing
`render_*` free functions; `monomyth-render-terse`'s `TerseRenderer` is a second, minimal medium
proving the seam supports more than one implementation. Both depend on `monomyth-core` and
`monomyth-contracts` only, and never on each other.

## Next: pixel-art

A **Sierra-style pixel-art frontend** — in the King's Quest / Space Quest lineage — is planned as a
second renderer over the same serialized world: a graphic adventure driven by the identical story
spine, cast, map, and item graph the text frontend already renders. This is forthcoming, not shipped:
no `monomyth-pixel` crate exists yet. It is architected for, not blocked on anything — the `Renderer`
trait and the render-agnostic contract are already in place; a new medium is one feature crate
implementing one trait, plus a composition-root wire-up.

## Frontends and generation never depend on each other

The serialized world is the only thing that crosses between generation/extraction and rendering. A
renderer never reaches into `monomyth-gen` or `monomyth-extract`, and neither reaches into a
renderer. This is what keeps adding a medium from touching anything upstream.

See [ADR-0001](https://github.com/Goldziher/monomyth/blob/main/adrs/0001-engine-plus-pluggable-frontends.md)
and [ADR-0020](https://github.com/Goldziher/monomyth/blob/main/adrs/0020-medium-agnostic-rendering-adapters.md).
