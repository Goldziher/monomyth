---
status: accepted
date: 2026-07-06
decision-makers: Na'aman Hirschfeld
---

# Commercial licensing: ship / reference namespaces

## Context and Problem Statement

The corpus mixes freely-redistributable material (public domain, permissive) with copyrighted and
NonCommercial sources we may only use as private reference. monomyth is a **commercial** product, so
these must never blur — a reference-only text must never be surfaced verbatim in shipped output.
How do we make that guarantee structural rather than aspirational?

## Decision Drivers

- Commercial product → NonCommercial and copyrighted sources are reference-only.
- The guarantee must be enforceable in code and CI, not just documented.
- We still want copyrighted works to *inform* generation (structure, priors) without redistribution.

## Considered Options

- Two tagged namespaces (`ship` / `reference`) with the invariant enforced at ingest, retrieval, and CI.
- Only ingest ship-safe material (discard reference sources entirely).
- Manual review / policy without structural enforcement.

## Decision Outcome

Chosen option: "two namespaces, enforced". Every source is declared in `corpus/manifest.json` with
`license` / `tier` / `namespace` / `domain`. `ship` = PD / CC0 / CC-BY / CC-BY-SA (share-alike
isolated); `reference` = copyrighted / NonCommercial. The **hard invariant**: a
`reference` / `noncommercial` / `copyright` source may never live in the `ship` namespace. Enforced
in three places: ingest guard, retrieval filter (`Filter::Eq("doc.namespace", "ship")`), and CI.

### Consequences

- Good, because reference content can inform generation while being provably non-redistributable.
- Good, because a `system` tier lets us ship uncopyrightable taxonomies even from copyrighted books.
- Bad, because every record carries provenance/licensing metadata and every retrieval path must be
  namespace-aware — real but modest overhead.

### Confirmation

The ledger validator asserts no non-`ship` tier sits in the `ship` namespace; shippable queries
carry the namespace filter; CI re-checks stored metadata against the ledger.

## More Information

Pre-1930-translation rule for PD-by-translation works: a modern translation of an ancient text
carries fresh copyright. Archive availability is never proof of public-domain status.
