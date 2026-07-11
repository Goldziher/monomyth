---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# User-uploaded media + `user` namespace/tier (architected now, implemented later)

## Context and Problem Statement

monomyth's licensing model (ADR-0005) recognizes exactly two trust domains: `ship` (PD/CC0/CC-BY/
CC-BY-SA, surfaceable) and `reference` (copyrighted/NonCommercial, priors-only). The roadmap adds a
third: users uploading their own media (their fiction, notes, world bibles) to ground generation or
extraction against. That content is licensed by the user, not monomyth, and must never be treated as
if it were curated ship material. User uploads are deferred (no fetchers, no UI yet), but the
`Namespace`/`Tier` enums in `crates/monomyth-knowledge/src/ledger.rs` are matched exhaustively by
tests today (e.g. `dangerous_tiers_are_reference_only`). How do we record this third trust domain now
so the binary licensing invariant does not have to be retrofitted under time pressure later?

## Decision Drivers

- The ship/reference split (ADR-0005) is a load-bearing commercial invariant enforced at ingest,
  retrieval, and `build_passage`; adding a third domain later without warning risks a match-arm or
  filter gap that leaks user content into a shippable path.
- User-uploaded content is licensed by the uploader, not monomyth — it cannot be assumed CC-anything
  and must default to non-surfaceable, mirroring `reference`'s posture, not `ship`'s.
- The plane model (ADR-0013) already designates `monomyth-knowledge` as the ledger's home across all
  three trust domains — this is a natural extension, not a new plane.
- Committing to the enum shape now — while `Namespace`/`Tier` have only two cases each — is far
  cheaper than discovering, mid-Phase-4 implementation, that every exhaustive `match` in the crate
  needs updating alongside new tests.

## Considered Options

- Add `Namespace::User` + `Tier::UserLicensed` now (enum + invariant + documented seam), implementation
  (upload flow, `user` collection ingestion) deferred to Phase 4.
- Model user uploads as a special case of `Namespace::Reference` with a note field, no enum change.
- Defer the whole decision — extend the enums only when upload implementation actually starts.

## Decision Outcome

Chosen option: "Add `Namespace::User` + `Tier::UserLicensed` now, implementation deferred", because
the two-namespace invariant is exhaustively matched and tested today, and extending it while the
codebase is small is materially cheaper and safer than a retrofit once more call sites exist.

`Namespace` gains a third variant, `User` (collection `"user"`, `as_wire()` token `"user"`), and
`Tier` gains `UserLicensed` (a source whose license is asserted by the uploader, not verified against
a public license text). The upload → ledger → collection seam is documented but not implemented: an
upload creates a `SourceEntry` with `namespace: Namespace::User`, `tier: Tier::UserLicensed`, and
whatever license string the uploader declares; it is written to the `user` collection, never to
`ship` or `reference`. **User content is non-surfaceable-by-default**, same posture as `reference` —
`KnowledgeQuery`'s shippable filter (`Filter::Eq("doc.metadata.namespace", "ship")`) excludes it
unless a project explicitly opts in per-source, and even then it is never auto-promoted to `ship`: a
human decision, not an automated one, is required to move a source's namespace. The
`dangerous_tiers_are_reference_only` test and equivalent enum-matching tests are extended to account
for three trust domains rather than two, so a match-arm gap surfaces at compile/test time, not at
ingest time in Phase 4.

Ingest, upload UI, and per-project promotion tooling are explicitly out of scope for this ADR —
Phase 4 of the roadmap implements them.

### Consequences

- Good, because the enum shape, the wire tokens, and the non-surfaceable-by-default posture are fixed
  before any upload code exists, so Phase 4 is a pure additive implementation, not a schema change.
- Good, because extending the invariant tests now (with zero real user data) catches every place that
  assumed exactly two namespaces while the fix is cheap.
- Good, because "never auto-promoted to ship" is recorded as policy before any promotion tooling
  exists, closing off an entire class of accidental-leak bug before it can be written.
- Bad, because the `user` variant and its tests exist with no runtime path that creates `User`-tagged
  entries yet — dead-ish code until Phase 4; mitigated by keeping the addition to the enum + tests
  only, no speculative ingest scaffolding.

### Confirmation

`cargo test -p monomyth-knowledge` includes `Namespace`/`Tier` covering `User`/`UserLicensed` in every
enumeration-based test (wire-token round-trip, `dangerous_tiers_are_reference_only`-equivalent). When
Phase 4 implements ingestion, the three-layer enforcement (ingest gate refuses non-`user` writes to
the `user` collection and vice versa; retrieval filter excludes `user` from default/shippable queries;
`build_passage` namespace check rejects surfacing a `user`-tagged passage in shipped output) is
extended to the third domain and covered by a test that a `user`-namespaced document never appears in
a `namespace = ship` filtered query result.

## Pros and Cons of the Options

### Add `Namespace::User` + `Tier::UserLicensed` now, implementation deferred

- Good, because it is decided and tested while the blast radius is smallest.
- Good, because it documents the non-surfaceable-by-default and no-auto-promotion policy before any
  code could violate it.
- Neutral, because the enum variant has no producer until Phase 4 — acceptable, it is data-shape
  scaffolding, not behavior.

### Model user uploads as `Namespace::Reference` with a note

- Bad, because it conflates two semantically different domains (curated reference corpus vs.
  arbitrary user content) under one tag, making it impossible to later treat them differently (e.g.
  per-project promotion applies to `user`, never to `reference`) without a breaking migration.
- Bad, because it hides the "licensed by the user, not us" distinction that justifies never
  auto-promoting it, inviting a future maintainer to treat it like curated reference material.

### Defer the whole decision

- Good, because it is zero work today.
- Bad, because it leaves every exhaustive `Namespace`/`Tier` match and every invariant test written
  for exactly two domains, guaranteeing a wider, riskier diff when Phase 4 starts under time pressure.

## More Information

Extends ADR-0005 (commercial licensing: ship/reference namespaces) with its natural third case.
Implementation is Phase 4 of the roadmap (`docs/roadmap.md`), after genre (Phase 2) and extraction
(Phase 3) so uploaded content has a stable extraction/grounding path to enter through. See
`crates/monomyth-knowledge/src/ledger.rs` for the `Namespace`/`Tier` enums this ADR extends.
