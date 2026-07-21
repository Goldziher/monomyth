---
status: accepted
date: 2026-07-21
decision-makers: Na'aman Hirschfeld
---

# quick-xml advisory posture under opendal services-fs

## Context and Problem Statement

The opendal 0.57 bump pulled `opendal-core`, which depends unconditionally on `quick-xml 0.39.4`. Two
advisories then landed against that version: RUSTSEC-2026-0194 (an O(N²) duplicate-attribute scan) and
RUSTSEC-2026-0195 (unbounded namespace-declaration allocation) — both denial-of-service risks when
parsing *untrusted XML*. `cargo deny` and `cargo audit` fail on them. The project rule is zero
tolerance for critical/high CVEs, but no fix is reachable and the vulnerable code may not be. How do we
keep the security gate honest without a fix we cannot obtain?

## Decision Drivers

- The dependency rule (CLAUDE.md): zero tolerance for critical/high CVEs — the gate must not be
  weakened casually.
- No upgrade exists: `opendal-core 0.57.0` pins `quick-xml = "^0.39.3"`, the fix is `quick-xml >= 0.41.0`,
  and 0.57.0 is the latest opendal — there is nothing to bump to.
- Reachability: `quick-xml` is exercised only by opendal's XML object-store services (S3/GCS/Azure list
  responses). monomyth enables only opendal's `services-fs` backend by default and under `--features
  acquire`; the cloud services sit behind the *reserved* `acquire-s3`/`acquire-gcs`/`acquire-azblob`
  features (ADR-0012), which are not wired to any code path yet.
- Precedent: the rsa "Marvin Attack" advisory (RUSTSEC-2023-0071) is already handled this way — ignored
  with a reachability justification because it too is reachable only via the reserved cloud features.

## Considered Options

- **Ignore both advisories in `deny.toml` and `.cargo/audit.toml`** with a reachability justification,
  scoped to the fs-only build, and a revisit trigger.
- **Fork or patch `opendal-core`** to bump `quick-xml`.
- **Drop opendal** and hand-roll the local blob cache.

## Decision Outcome

Chosen option: "ignore both advisories with a reachability justification", because the vulnerable
parsers are unreachable in every build monomyth actually ships, no fix is obtainable, and the two
alternatives are disproportionate to a DoS-only risk on an inert code path.

The vulnerable `quick-xml` parsers run only when opendal parses an XML object-store list response. With
only `services-fs` compiled in, no runtime path reaches them: the filesystem backend never parses XML.
`quick-xml` differs from the rsa case in one way — `opendal-core` pulls it *unconditionally*, so it is
in `cargo deny`'s feature-aware graph (rsa is not), which is why it must be ignored in **both**
`deny.toml` and `.cargo/audit.toml`, not audit alone. Each ignore carries the reachability rationale
inline, and both gates are green after the change.

### Consequences

- Good, because the security gate passes truthfully: the advisories are acknowledged, justified, and
  scoped, not silenced blindly.
- Good, because it mirrors the existing rsa handling, so the ignore list stays legible and consistent.
- Bad, because an ignored advisory is a standing liability if a cloud acquire feature is ever wired up
  without revisiting this — mitigated by the explicit revisit trigger below and by the reserved-feature
  status of those backends.
- Bad, because it depends on a manual reachability argument rather than a tool proving the path dead;
  accepted because the feature gating makes the argument robust and auditable.

### Confirmation

`cargo deny check advisories` and `cargo audit` both pass with the two advisories ignored. The ignore
entries in `deny.toml` and `.cargo/audit.toml` carry the reachability justification and the revisit
trigger. **Revisit when:** opendal bumps `quick-xml` to ≥ 0.41.0 (drop the ignores), or any
`acquire-s3`/`acquire-gcs`/`acquire-azblob` feature is wired to a real code path (re-evaluate, since the
XML parsers would then become reachable).

## Pros and Cons of the Options

### Ignore with a reachability justification

- Good, because it is proportionate: a DoS advisory on an uncompiled-reachable path, no fix available.
- Good, because it reuses the established rsa pattern and keeps the gate green without weakening it
  globally.
- Neutral, because it must be revisited on two specific triggers, which are documented inline.

### Fork or patch `opendal-core` to bump `quick-xml`

- Bad, because `quick-xml 0.41` is a semver-incompatible bump for `opendal-core`'s `^0.39.3` requirement;
  a fork would carry non-trivial maintenance and re-verification for a risk that is not reachable.

### Drop opendal, hand-roll the blob cache

- Bad, because it discards ADR-0012's entire local-FS-now / cloud-later abstraction to sidestep a
  single unreachable transitive advisory — a large, permanent cost for a temporary, inert one.

## More Information

Relates to ADR-0012 (OpenDAL corpus blob storage — the reserved cloud features are what make the XML
path reachable at all) and mirrors the rsa reachability handling recorded in `.cargo/audit.toml`.
Revisit on either trigger in the Confirmation.
