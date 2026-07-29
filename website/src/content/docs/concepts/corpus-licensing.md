---
title: Corpus & licensing
description: The ship/reference namespace model, enforced at ingest, retrieval, and CI — the hard invariant that keeps generated output redistributable.
---

monomyth grounds generation in a corpus of mythology, folklore, esoterica, and SF/F, retrieved via
[xberg](https://crates.io/crates/xberg). monomyth's output is permissively (MIT) licensed and may be
used for any purpose, including commercially, so the corpus mixes freely-shippable and reference-only
material that must never blur. This is enforced, not aspirational.

## Two namespaces

Every source is declared in `corpus/manifest.json` with a `license`, `tier`, `namespace`, and
`domain`. Nothing is fetched or ingested that is not declared there first.

| Namespace | Licenses | May be surfaced verbatim? |
|---|---|---|
| `ship` | PD / CC0 / CC-BY / CC-BY-SA | Yes |
| `reference` | Copyrighted or NonCommercial | No — informs generation only |

`CC-BY-SA` material is isolated within `ship` so its share-alike obligation cannot contaminate PD/CC0
material.

## The hard invariant

A source tagged `reference` / `noncommercial` / `copyright` may never live in the `ship` namespace.
This is enforced in three places:

1. **At ingest** — the ingest gate refuses to write a non-`ship` source into a shippable collection.
   `monomyth ingest --reference` is the inverse gate, admitting *only* `reference`-namespace sources
   into the reference collection.
2. **At retrieval** — shippable queries filter with
   `Filter::Eq("doc.metadata.namespace", "ship")`, the xberg filter IR (the filter whitelist admits
   only `doc.metadata.*` for free-form tags).
3. **In CI** — `monomyth corpus audit` checks stored document metadata against the ledger.

## The `system` tier

The `system` tier marks an uncopyrightable idea or taxonomy (a list of stage or role names, for
example) that monomyth independently encodes — ship-safe even when the source book itself is in
copyright. A `system`-tier artifact reproduces **no prose** from its source. This is how the
mythology frameworks (Campbell, Propp, Polti — themselves drawn from copyrighted scholarship) and the
build-time-synthesized "law" artifacts can ship at all: the taxonomy is encoded, the source's wording
never is.

## Pre-1930 translations only

A modern translation of an ancient text carries its own, fresh copyright — translating Homer in 1990
does not put that translation in the public domain just because the *Odyssey* itself is ancient.
Public-domain-by-translation claims are honored only for pre-1930 translations, and the translator
and year are recorded and verified before a source ships. Availability on an archive is never treated
as proof of public-domain status.

## Provenance

Every document and chunk carries a source id, URL, retrieval date, and a content checksum. This
travels with the record end to end, not just at ingest time.

## Build-time synthesis stays inside the gate

The `monomyth-synthesis` crate drafts law candidates from `reference`-namespace priors. Every step is
gated:

- Reference ingest writes only to the reference collection, never `ship`.
- Synthesized artifacts are stamped `Tier::System` — idea/taxonomy only, no prose.
- Every synthesis is human-reviewed before commit — an empty `reviewed_by` is refused by the loader,
  so nothing ships unreviewed.
- A fourth, synthesis-specific layer — an anti-leak shingle check — refuses a drafted candidate that
  shares any 8-word run with a reference passage, a machine-checked backstop against a distillation
  model reproducing source wording.

See [ADR-0005](https://github.com/Goldziher/monomyth/blob/main/adrs/0005-commercial-licensing-ship-reference.md)
and [ADR-0016](https://github.com/Goldziher/monomyth/blob/main/adrs/0016-build-time-methodology-and-law-synthesis.md).
