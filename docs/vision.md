# Vision

## Thesis

monomyth is a **configurable, layered engine for narrative structure**. It moves between
*unstructured narrative* and *structured narrative representation* — in **both directions** — for
**any medium**, grounded in source material.

- **Generation**: structure → prose/game/media (contract → medium). This exists today, partially.
- **Extraction**: prose/media → structure (medium → contract). This is new, and absent today.

They are not two features bolted together. They are **inverse transforms over one shared contract**.
Building extraction is not "a new feature" — it is completing the other half of a transform the
engine was always meant to have.

monomyth is not a mythology text-adventure generator that happens to use a contract internally. It is
a narrative-structure engine whose first grounding domain is comparative mythology, and whose first
medium is text adventures.

## Framing 1: the contract is the pivot

At the center sits a single, medium-agnostic, serializable model — `monomyth-core`'s `World`/`Story`.
Every transform reads or writes this contract; nothing bypasses it.

```text
        extraction                    generation
text/media ────────────▶  World  ────────────▶ text/media
                        (the contract)
```

- The contract is **pure data**: no IO, no async, no LLM calls, no config types, no genre types. It
  depends on nothing.
- Extraction and generation are inverse functions over it, which is what makes round-tripping
  (extract → regenerate, generate → re-extract) a meaningful thing to test.
- Rendering is a thin, downstream, contract-only adapter — not a place where medium-specific concerns
  leak back into the contract.

Because the contract is medium-agnostic, "any medium" is not aspirational marketing — it is a
structural consequence of the pivot. Prose, a text adventure, a LitRPG stat sheet, a detective-fiction
clue graph, or a UI flow are all just different renderings of, or different sources for, the same
`World`/`Story` shape.

## Framing 2: the working method

Everything the build-time side of the engine does follows one repeatable procedure:

```text
map/extract concept  →  synthesize into rule(s)  →  translate into configuration
```

1. **Map/extract concept.** Ground a domain concept in source material via the xberg RAG layer —
   scholarship, reference texts, curated corpora.
2. **Synthesize into rule(s).** Distill the concept into a structured, language-neutral rule or
   taxonomy — the kind of thing a computer can check, not prose.
3. **Translate into configuration.** Turn the rule into generation/extraction vocabulary or tunable
   parameters that the run-time transforms consume.

This is a **build-time plane** producing rules and configuration, feeding a **run-time plane**
(extraction ⇄ generation) that consumes them. The two planes have different lifecycles: build-time
output is synthesized once (or occasionally re-synthesized) and reviewed; run-time transforms execute
per request, deterministically where structure is concerned.

## Where mythology sits

Comparative mythology is not the product — it is the **first instance of the method**. Campbell's
monomyth, Propp's functions, Polti's situations, and the character-role frameworks were mapped from
scholarship, synthesized into the `artifacts/frameworks/*.json` taxonomies, and translated into the
vocabulary `monomyth-core` and `monomyth-gen` use to build worlds.

The same method applies to any other concept domain:

- Myth-theory "laws" (Laurasian/Gondwanan arc structure, binary-opposition mediation, trifunctional
  casting) synthesized from reference-only scholarship into ship-safe, `system`-tier artifacts.
- Genre models, synthesized the same way, targeting output at a medium/tone.
- Extraction schemas, mapping how unstructured text maps onto contract shapes.

Mythology stays as the flagship grounding domain and the default vocabulary. It is not architecturally
privileged — a future concept domain does not need to touch mythology's code path to exist.

## What changes vs. the old framing

| Old framing | New framing |
|---|---|
| "A mythology text-adventure generator" | A configurable, layered engine for narrative structure |
| Generation only | Generation **and** extraction — inverse transforms over one contract |
| Text adventures as the target | Any medium; text adventures as the first rendering |
| Mythology as the subject | Mythology as the first instance of a general method |
| Frameworks as a fixed schema | Frameworks as the output of a repeatable `map → synthesize → configure` procedure, extensible to new concept domains |
| Config as scattered constants | Config as a first-class, cross-cutting, layered-override concern |

Nothing about the shipped engine's behavior changes retroactively — the mythology-grounded text
adventure it produces today is still correct output. What changes is the architecture around it: the
pieces that made mythology-grounded generation work are being named, generalized, and given seams so
the same machine can extract as well as generate, and can target new concept domains and media without
rearchitecting.

## Audience

This document is for anyone who wants to understand what monomyth is building toward: contributors,
reviewers, and future sessions picking the project back up. For the concrete architecture, see
[`architecture.md`](./architecture.md). For the method in practice, see [`methodology.md`](./methodology.md).
For the phased plan, see [`roadmap.md`](./roadmap.md).
