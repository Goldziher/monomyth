# corpus — license ledger

monomyth grounds generation in a corpus of comparative mythology, folklore, esoterica, and SF/F.
This directory holds the **license & namespace ledger**. The domain schema lives in
[`../artifacts/frameworks/`](../artifacts/frameworks), and fetching / chunking / embedding /
retrieval are owned by the Rust `monomyth-knowledge` crate (xberg) — see [`../adrs`](../adrs).

## `manifest.json` — the license & namespace ledger

Every source is declared here with `license`, `tier`, `namespace`, and `domain`. It is the
provenance ground truth and the enforcement point: monomyth's output is permissively (MIT) licensed
and may be used for any purpose, including commercially, so reference-only material is never surfaced:

- `ship` — PD / CC0 / CC-BY / CC-BY-SA (share-alike isolated). May be surfaced verbatim.
- `reference` — copyrighted or NonCommercial. Informs generation (structure/priors) but is never
  redistributed / surfaced verbatim.

The hard invariant: a source tagged `reference` / `noncommercial` / `copyright` may never live in the
`ship` namespace. Enforced at ingest, at retrieval (xberg `Filter` on `doc.metadata.namespace`), and in CI.
The `system` tier marks an uncopyrightable taxonomy (e.g. a list of stage names) as ship-safe even
when the source book is in copyright. See [ADR-0005](../adrs/0005-commercial-licensing-ship-reference.md).

## Domain schema — `../artifacts/frameworks/`

Hand-encoded, validated analytic frameworks — a 3-tier plot model (Campbell → Propp + Polti →
Thompson) plus a character model (Propp role × Greimas actant × archetype), bound by crosswalks.
Language-neutral JSON; the Rust `story` module reads it directly. See
[ADR-0004](../adrs/0004-mythology-frameworks-as-domain-schema.md).
