---
title: Introduction
description: What monomyth is, and why narrative structure is medium-agnostic.
---

monomyth is a **configurable, layered engine for narrative structure**. It moves between
unstructured narrative and a structured, medium-agnostic **world/story model** — in both
directions:

- **Generation** — structure → prose/game/media (contract → medium). This exists today.
- **Extraction** — prose/media → structure (medium → contract). A proof-of-concept exists; the full
  text-to-structure slice is forthcoming.

These are not two features bolted together. They are inverse transforms over one shared contract.

monomyth is not a mythology text-adventure generator that happens to use a contract internally. It
is a narrative-structure engine whose first grounding domain is comparative mythology, and whose
first medium is text adventures.

## The contract is the pivot

At the center sits a single, medium-agnostic, serializable model — the `World`/`Story` type in
`monomyth-core`. Every transform reads or writes this contract; nothing bypasses it.

```text
        extraction                    generation
text/media ────────────▶  World  ────────────▶ text/media
                        (the contract)
```

The contract is pure data: no IO, no async, no LLM calls, no config types, no genre types. It
depends on nothing. Extraction and generation are inverse functions over it, which is what makes
round-tripping (extract → regenerate, generate → re-extract) a meaningful thing to build toward.
Rendering is a thin, downstream, contract-only adapter — not a place where medium-specific concerns
leak back into the contract.

Because the contract is medium-agnostic, "any medium" is a structural consequence of the pivot, not
aspirational marketing. Prose, a text adventure, a LitRPG stat sheet, a detective-fiction clue graph,
or a graphic-adventure scene are all different renderings of, or different sources for, the same
`World`/`Story` shape.

## Why medium-agnostic matters

A core engine produces the world/story model; pluggable frontends render it. The model carries no
colors, glyphs, or screen coordinates — nothing that would tie it to one medium. That is what lets a
second frontend (a Sierra-style pixel-art game, in the King's Quest / Space Quest lineage) drop in
later as a peer of the text frontend, over the exact same serialized world, cast, map, and item
graph — not a rewrite.

The same reasoning applies to genre. LitRPG, fantasy, sci-fi, detective fiction, and classic
interactive fiction are not separate engines; they resolve from a configuration profile over the
same procedural structure and the same corpus discipline. See
[Genres](/monomyth/concepts/genres/) and [Frontends](/monomyth/concepts/frontends/).

## Grounded, not invented

The story vocabulary is distilled from comparative-mythology scholarship, not invented from
intuition: Campbell's monomyth stages, Propp's functions, Polti's dramatic situations, and Thompson's
motif index, bound by crosswalks into a three-tier plot model plus a character model. See
[Frameworks](/monomyth/reference/frameworks/).

## Where to go next

- [Installation](/monomyth/start/installation/) — build the workspace.
- [Quickstart](/monomyth/start/quickstart/) — generate, play, edit, and compose a world from the CLI.
- [Architecture](/monomyth/concepts/architecture/) — how the crates are decomposed.
