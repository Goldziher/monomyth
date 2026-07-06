---
priority: critical
---

# Licensing & provenance

monomyth is a **commercial** product. The corpus mixes freely-shippable and reference-only material,
and the two must never blur. This rule is non-negotiable.

- **Every source is declared in the ledger** (`corpus/manifest.json`) with `license`, `tier`,
  `namespace`, and `domain`. Nothing is fetched or ingested that is not declared.
- **Two namespaces:**
  - `ship` — PD / CC0 / CC-BY / CC-BY-SA only. May be surfaced verbatim in generated output.
    Isolate CC-BY-SA so its share-alike obligation cannot contaminate PD/CC0 material.
  - `reference` — copyrighted or NonCommercial (e.g. Dúchas, Perseus, TV Tropes, in-copyright
    canon). Informs generation (structure/priors) but is **never** redistributed or surfaced verbatim.
- **The hard invariant:** a source tagged `reference` / `noncommercial` / `copyright` may **never**
  live in the `ship` namespace. Enforce it in three places:
  1. at ingest (refuse to write a non-`ship` source into a shippable collection),
  2. at retrieval (shippable queries filter `Filter::Eq("doc.metadata.namespace", "ship")` — the
     xberg-rag filter whitelist admits only `doc.metadata.*` for free-form tags),
  3. in CI (a check over the ledger + stored metadata).
- **The `system` tier** marks an uncopyrightable idea/taxonomy (e.g. a list of stage or role names)
  that we independently encode — ship-safe even when the source book is in copyright. We reproduce
  **no prose** from such a source.
- **Pre-1930 translations only** for PD-by-translation works: a modern translation of an ancient text
  carries fresh copyright. Record the translator year and verify before shipping.
- **Provenance on every record:** source id, URL, retrieval date, and a content checksum travel with
  each document/chunk. Availability on an archive is not proof of public-domain status — verify.
