---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# Rust corpus acquisition pipeline and stored provenance

## Context and Problem Statement

The manifest (`corpus/manifest.json`, ADR-0005) declares the corpus sources, and `monomyth-knowledge`
can ingest and retrieve (ADR-0006, ADR-0007), but nothing acquires the material: `corpus/raw/` is
empty and ingest only accepts text handed to it by hand. The retired Python prototype (ADR-0006) had
the fetch → normalize flow. How do we populate the corpus in Rust, and how do we carry the provenance
ADR-0005 requires on every stored record?

## Decision Drivers

- ADR-0006 consolidated on Rust; the acquisition path must be Rust, not a revived Python layer.
- ADR-0005 requires source id, URL, retrieval date, and a content checksum to travel with every
  stored document/chunk — today ingest stores only the license tags.
- The networked acquisition must not burden the generation path: `monomyth-gen` depends on
  `monomyth-knowledge` and must never pull an HTTP stack.
- Reproducibility: a re-run against the same sources should be cache-hitting and stable.

## Considered Options

- A feature-gated `acquire` module inside `monomyth-knowledge` (fetch/normalize/ingest), off by
  default; xberg keeps ownership of chunking.
- A separate `monomyth-corpus` crate depending on `monomyth-knowledge` + an HTTP stack.
- Fetch logic living in `monomyth-cli` directly.

## Decision Outcome

Chosen option: "feature-gated `acquire` module in `monomyth-knowledge`". The corpus layer stays one
crate (ADR-0006), but the networked half lives behind an `acquire` cargo feature that is off by
default, so `monomyth-gen` never compiles an HTTP stack. The module ports the retired Python flow:
`http` (cached GET under `corpus/raw/<hash>.bin`, retry/backoff, gzip, SHA-256), `normalize`
(strip Project-Gutenberg boilerplate, unwrap hard-wrapped paragraphs), and a small `Fetcher` trait
with per-family impls (Gutenberg/Gutendex, HuggingFace datasets-server JSON rows, archive.org). The
pipeline produces clean `full_text` and hands it to `Knowledge::ingest`; **xberg's Semantic chunker
(already wired) does the chunking** — we do not re-port a chunker (ADR-0007).

Provenance becomes a first-class part of ingest (always on, not feature-gated): `IngestInput` carries
`url` / `checksum` / `retrieved`, and `Knowledge::ingest` writes them into each document's metadata
alongside the existing `source_id` / `namespace` / `license` / `tier` / `domain`. `corpus/raw/**`
stays gitignored and regenerable (data, not source).

### Consequences

- Good, because the corpus can be built from a command and every record is provenance-complete.
- Good, because the generation path stays lean — `reqwest`/`sha2` compile only under `acquire`.
- Good, because stored provenance is exactly what makes ADR-0005's third enforcement point (a CI
  check over ledger + stored metadata) runnable — see ADR-0005 and the audit in the knowledge crate.
- Bad, because feature unification means building `monomyth-cli` (which enables `acquire`) compiles
  the HTTP stack; acceptable, since the CLI is the top of the graph.

### Confirmation

`monomyth corpus build --source <id> --limit N` fetches, normalizes, and ingests ship-safe sources;
a subsequent `retrieve` returns passages carrying `url` + `checksum` + `retrieved`. Round-trip tests
in `monomyth-knowledge` assert provenance is stored and read back. The default-feature build of
`monomyth-gen` has no HTTP dependency in its graph.

## Pros and Cons of the Options

### Separate `monomyth-corpus` crate

- Good, because the HTTP stack is physically isolated from the knowledge crate.
- Bad, because it splits the corpus layer ADR-0006 deliberately unified, and duplicates the ledger
  and ingest seams.

### Fetch logic in `monomyth-cli`

- Bad, because the reusable fetch/normalize library code would be trapped in a binary and untestable
  as a library surface.

## More Information

Extends ADR-0006 (Rust consolidation) and depends on ADR-0007 (xberg owns chunking) and ADR-0005
(licensing + provenance). Fine-grained ATU/Thompson enrichment from the CC-BY-SA `trilogy` dataset is
separate downstream work.
