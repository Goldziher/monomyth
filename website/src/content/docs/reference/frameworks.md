---
title: Frameworks
description: The 3-tier plot model, the character model, and the crosswalks binding them — the domain schema monomyth-frameworks reads.
---

The story vocabulary is not invented — it is distilled from comparative-mythology scholarship into
validated, language-neutral JSON under `artifacts/frameworks/`. `monomyth-frameworks` reads these
artifacts as typed enums and crosswalk accessors; `monomyth-gen`'s procedural passes and content
prompts target the crosswalks.

## The 3-tier plot model

| Tier | Source | Granularity |
|---|---|---|
| Macro-arc | Campbell's monomyth | The hero's-journey stages (17 canonical stages) |
| Meso | Propp's functions + Polti's dramatic situations | Propp's 31 functions, Polti's 36 situations |
| Micro | Thompson's motif index | Fine-grained narrative motifs |

## The character model

Propp role × Greimas actant × archetype — three established typologies for narrative roles, bound
together rather than picking one.

## Crosswalks

Arc, plot, and character crosswalks bind the tiers and the character model to each other, so a
procedural pass targeting a Campbell stage can resolve down to a consistent Propp function, Polti
situation, and cast role rather than picking each layer independently.

## Validation

`monomyth-frameworks`'s parity tests enforce:

- Canonical counts (Propp 31, Polti 36, Campbell 17, …).
- Crosswalk referential integrity — every crosswalk entry resolves to a real node on both sides.
- The license-ledger invariant, described below.

Any new framework artifact must pass the same validation.

## Licensing note

Some source frameworks are themselves copyrighted works. Only the uncopyrightable system or label —
the `system` tier, a list of stage names, a taxonomy — is encoded; no prose is reproduced from the
source scholarship. See [Corpus & licensing](/monomyth/concepts/corpus-licensing/).

ATU tale-types and the fine-grained Thompson motif table are sourced separately, from the CC-BY-SA
`trilogy` dataset at fetch time rather than hand-encoded, and are isolated in the `ShareAlike` tier so
their share-alike obligation cannot contaminate PD/CC0 material.

## Framework artifacts vs. config

Framework artifacts (`artifacts/frameworks/*.json`) are build-time ground-truth *vocabulary*: the
code conforms to the artifact, it is immutable at run time, and only a reviewed commit can change it.
This is a different thing from `monomyth-config`'s run-time *tunable parameters*, which may select or
weight how a transform uses an artifact but never edit the artifact itself. See
[Configuration](/monomyth/reference/configuration/).

See [ADR-0004](https://github.com/Goldziher/monomyth/blob/main/adrs/0004-mythology-frameworks-as-domain-schema.md).
