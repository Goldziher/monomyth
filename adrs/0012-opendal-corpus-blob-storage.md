---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# OpenDAL for corpus blob storage: local FS in dev, object-storage buckets in the cloud

## Context and Problem Statement

ADR-0010 caches raw corpus fetches to `corpus/raw/<hash>.bin` through `tokio::fs`. The deployment
shape has since clarified: the same corpus blob storage — the raw-fetch cache **and** the
download-for-inspection area for reference/unverified sources — must work both locally (developer
machine, single binary) and in the cloud (object-storage buckets, potentially many instances). It
must also keep **ship-safe** material physically separated from **reference/inspect-only** material
on whatever backend it runs against. Storage size and egress cost are explicitly **not** a
constraint; the constraint is one code path over both backends plus trust separation.

A `tokio::fs`-only cache cannot address a bucket without a second, parallel storage implementation.

## Decision Drivers

- One storage code path for local FS (dev) and cloud object storage (prod) — backend chosen by
  config, not by a code fork.
- Physical ship/reference separation on disk *and* in the bucket, so reference/unverified downloads
  can never be mistaken for shippable material.
- Keep the acquisition path optional and off the generation graph (ADR-0010) — the storage
  dependency must be feature-gated under `acquire`.
- The vector store (sqlite-vec embedded / pgvector server, ADR-0011) is a separate concern; this
  decision covers blob/raw text only, not the vector index.

## Considered Options

- **Apache OpenDAL** as the blob-storage abstraction: a single `Operator`, configured for local FS
  in dev and S3/GCS/Azure buckets in the cloud, with trust separation by path prefix.
- **Direct `tokio::fs`** (ADR-0010 as written): no cloud path; a bucket backend would be a second
  hand-written storage layer.
- **A hand-rolled `trait BlobStore` over fs + an S3 SDK**: reinvents exactly what OpenDAL is, and we
  would maintain the S3/GCS/Azure adapters ourselves.
- **The `object_store` crate**: viable and lighter, but narrower service coverage and a less uniform
  API than OpenDAL for the fs-plus-many-clouds spread we want.

## Decision Outcome

Chosen option: "Apache OpenDAL". The corpus raw-fetch cache and the reference/inspect area are read
and written through an `opendal::Operator`. The operator is selected by configuration/environment:
a local-filesystem service rooted at `corpus/raw/` in development, and an S3/GCS/Azure bucket
service in the cloud — with **no change to any call site**. Trust separation is expressed as path
prefixes within the operator (`ship/…` for verified ship-safe material, `reference/…` for
download-for-inspection-only material that is never ingested into the ship collection). OpenDAL and
its cloud service backends are feature-gated under `acquire` (cloud services behind their own
sub-features so a local build pulls only the FS service).

This **amends ADR-0010's storage mechanism** (the `tokio::fs` `corpus/raw/<hash>.bin` cache); the
rest of ADR-0010 (feature-gated acquisition, xberg owns chunking, provenance on every record)
stands. It does **not** affect ADR-0011 (the vector store).

### Consequences

- Good, because local and cloud storage share one code path — dev parity with prod, backend by
  config.
- Good, because ship/reference separation is enforced by prefix on every backend, reinforcing the
  ADR-0005 invariant at the storage layer.
- Good, because we consume a mature, widely-used Apache project rather than maintaining per-cloud
  adapters (dependency-awareness rule).
- Bad, because `acquire` gains a non-trivial dependency; mitigated by feature-gating (off by
  default, cloud services behind their own features) so `monomyth-gen` and default builds never pull
  it.
- Neutral, because storage cost is not a design constraint here (per the deployment brief), so the
  choice optimizes for one code path and trust separation, not footprint.
- Bad, because the reserved `acquire-s3`/`acquire-gcs`/`acquire-azblob` sub-features make OpenDAL's
  cloud-service crates resolvable, which drags a known advisory (RUSTSEC-2023-0071, the `rsa` "Marvin
  Attack" timing side-channel, 5.9 medium, no upstream fix) into `Cargo.lock` via `reqsign`. It is
  **never compiled** into the default or `--features acquire` build (`cargo-deny`'s feature-aware
  graph does not surface it; only `cargo audit`, which scans the raw lockfile, does), so it is ignored
  in `.cargo/audit.toml` with a reachability note. This must be revisited before any cloud acquire
  sub-feature is actually wired to a live bucket — at which point a non-`rsa` signer is a prerequisite.

### Confirmation

The corpus cache reads/writes go through a single `Operator`; a local build runs on the FS service
with no cloud dependency compiled in, and switching to a bucket is a config change with no call-site
edit. Ship-safe and reference/inspect material resolve to distinct prefixes, verified by tests that
assert a reference-namespace fetch never writes under the `ship/` prefix and is never ingested into
the ship collection.

## More Information

Amends ADR-0010 (corpus acquisition — storage mechanism). Independent of ADR-0011 (vector-store
backends). Builds on ADR-0005 (ship/reference licensing invariant), which the prefix separation
enforces at the storage layer.
