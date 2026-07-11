---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# Extraction subsystem (monomyth-extract)

## Context and Problem Statement

Generation (`monomyth-gen`) is the structure→text half of the contract; extraction — text→`World` —
is the inverse and does not exist yet. monomyth's near-term focus is deepening generation grounding,
so extraction stays deferred, but the plane model (ADR-0013) and the frozen contract (ADR-0014) are
decided now. How do we place the extraction seam so it lands, when implemented, without forcing a
contract change or duplicating the write/validate surface generation already built?

## Decision Drivers

- Extraction and generation are inverse transforms over one pivot (`monomyth-core::World`) and must
  read the same framework vocabulary (`monomyth-frameworks`) so a beat means the same thing on both
  sides of the round trip.
- The contract must never be mutated ad hoc to make extraction "fit" — ADR-0014's `SCHEMA_VERSION`
  gate is the only sanctioned path for a contract change.
- Generation already has a validated write surface (`NarrativeEdit` + `World::validate`) and a
  content fill seam (`Content<T>`); extraction should reuse both, not invent parallel ones.
- Extraction is inherently config- and genre-driven (what counts as a "beat" depends on target genre)
  and needs grounding via `monomyth-knowledge` to map free text onto framework vocabulary.
- Near-term priority is generation grounding, not extraction; the seam must be architected without
  requiring the implementation now.

## Considered Options

- A new `monomyth-extract` crate exposing an `Extractor` trait (P-TRANSFORM plane), producing `World`
  values exclusively through `NarrativeEdit`/`World::validate`, implementation deferred.
- Extend `monomyth-gen` with a reverse code path in the same crate.
- Defer the decision entirely — no crate, no trait, revisit when extraction work actually starts.

## Decision Outcome

Chosen option: "A new `monomyth-extract` crate exposing an `Extractor` trait, implementation
deferred", because it lets the round-trip seam be architected now — while the contract is still small
and easy to reason about — without committing to any extraction algorithm.

`monomyth-extract` sits in the P-TRANSFORM plane (ADR-0013) beside `monomyth-gen`. Its `Extractor`
trait (declared in `monomyth-contracts` per ADR-0013's trait-first rule) takes source text plus
config/genre context and produces a **valid** `World`: it builds the graph exclusively through
`NarrativeEdit::apply_edits` (never touching `NarrativeStructure` internals directly) and gates every
result through `World::validate`/`world.validate()`, the same checks generation's output must pass.
Content slots it cannot fill from source text are left as `Content::Empty`, reusing the existing
generation fill seam (ADR-0002) rather than inventing an extraction-specific one. Extraction is
grounded via `monomyth-knowledge` retrieval (mapping source passages onto `monomyth-frameworks`
vocabulary) and is driven by config/genre (ADR-0015, ADR-0017), never by hard-coded heuristics.

**The hard rule:** `monomyth-extract` cannot force a change to `monomyth-core`. If a real source text
cannot be represented by the current contract, that is evidence for an ADR-0014-gated
`SCHEMA_VERSION` bump — proposed, reviewed, and versioned — never a silent edit to make one extractor
pass. Implementation itself is out of scope for this ADR (Phase 3 of the roadmap); this ADR fixes the
crate boundary and the constraint so the contract does not churn once extraction lands.

### Consequences

- Good, because extraction and generation share one validated write surface and one content-fill
  seam, so neither transform can produce a `World` the other cannot also produce.
- Good, because the contract-change gate is decided before any extraction code exists, closing off
  the failure mode of an extractor quietly bending the schema to fit one example text.
- Good, because deferring implementation costs nothing now — the trait and crate are cheap scaffolding
  that de-risks Phase 3 without pulling engineering effort off generation grounding.
- Bad, because the trait signature is speculative until a real extraction slice is attempted; it may
  need revision once genre-driven parsing proves out — acceptable since no implementation depends on
  it yet.

### Confirmation

A round-trip test (extract a minimal example text → `World` → generate from that `World`) exercises
the shared write/validate surface once the crate has a first impl. Any case where extraction cannot
represent source structure in the current contract is written up as an ADR-0014 addendum proposing a
`SCHEMA_VERSION` bump — never patched directly into `monomyth-core`. Code review enforces that
`monomyth-extract` never depends on `monomyth-core` internals beyond its public `NarrativeEdit`/
`validate` surface.

## Pros and Cons of the Options

### New `monomyth-extract` crate, `Extractor` trait, deferred implementation

- Good, because it fixes the seam (crate boundary, write surface, validation gate) while the contract
  is still cheap to change, rather than retrofitting it after generation has grown more surface area.
- Good, because it matches the plane model: extraction and generation are peer P-TRANSFORM impls.
- Neutral, because the trait may need a signature revision once a real slice is implemented (Phase 3).

### Reverse code path inside `monomyth-gen`

- Bad, because it conflates two independently-evolving transforms in one crate and blurs the
  P-TRANSFORM plane's "one trait per direction" boundary from ADR-0013.
- Bad, because it makes it easy to informally share internal helpers that aren't the sanctioned
  `NarrativeEdit`/`validate` surface, reintroducing the parallel-write-path risk this ADR closes off.

### Defer entirely — no crate, no trait yet

- Good, because it is the least work today.
- Bad, because it leaves the round-trip seam unarchitected; the first real extraction attempt would
  either invent an ad hoc write path or discover contract gaps with no established gate to route them
  through, risking exactly the silent contract churn this ADR prevents.

## More Information

Inverse of ADR-0002 (hybrid generation: structure then content-fill — extraction is content/structure
*inward*, generation *outward*, over the same `Content` seam). Constrained by ADR-0014 (the contract
pivot and its `SCHEMA_VERSION` gate). Instantiates ADR-0013 (P-TRANSFORM plane, trait-first seam).
Implementation is Phase 3 of the roadmap (`docs/roadmap.md`), gated on Phase 2 genre targeting so
extraction consumes a stable schema/prior selector.
