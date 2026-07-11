---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# The contract is the extraction⇄generation pivot; stability & versioning rules

## Context and Problem Statement

monomyth is growing a second, symmetric transform: **extraction** (unstructured text → `World`)
alongside today's **generation** (`World` → text/media). Both must move through the same serializable
domain model, or the engine ends up with two divergent notions of "story." What guarantees does
`monomyth-core`'s `World`/`Story` need to safely serve as the pivot both directions read and write,
and what happens when a transform needs a shape the contract doesn't have?

## Decision Drivers

- Extraction and generation must target *one* model, not parallel ad hoc representations, or the two
  transforms drift and round-tripping (text → `World` → text) becomes meaningless.
- ADR-0001 already requires the contract to be render-agnostic; it must now also stay
  transform-agnostic (medium-agnostic *and* direction-agnostic).
- Determinism (ADR-0002, ADR-0003): `(seed, action-log)` replay and snapshot tests depend on the
  contract's shape and serialized form being stable across releases.
- A future transform (extraction, a new renderer, a new genre) will inevitably want a shape the
  contract doesn't have yet; that pressure must be resolved through a gate, not a silent edit.

## Considered Options

- Freeze `monomyth-core` as the single pivot: both transforms are inverse functions over it; any
  shape change is a versioned, deliberate act.
- Give extraction its own intermediate model, converted into `World` at the end.
- Let each transform mutate/extend `World` directly as its needs arise, informally.

## Decision Outcome

Chosen option: "freeze `monomyth-core` as the single pivot". `World`/`Story` is the one
medium-agnostic contract; **extraction** (text → `World`, ADR-0018) and **generation**
(`World` → media, ADR-0002) are inverse transforms over it, not two models bridged by a converter.
The contract stays **pure**: no IO, no async, no LLM calls, no config types, no genre types — it is
the shared serializable fact both directions produce and consume, never the place either direction's
mechanics leak into.

The `Content::Empty`/`Content::Filled` seam (`crates/monomyth-core/src/content.rs`) is the shared fill
mechanism both directions use, not a generation-only device: extraction populating a `World` from text
fills content slots the same way the LLM content pass does, and a value in either direction carries a
`Provenance` recording where it came from.

**Versioning rule:** any change to the contract's serialized shape bumps
`SCHEMA_VERSION` (`crates/monomyth-core/src/world.rs`, currently `2`) and is documented at the
constant — the ADR-0003 precedent (`1 → 2` for the branching-narrative-structure change) is the
pattern to repeat. A version bump is a breaking-change gate: `World::from_json_checked` rejects a
world stamped with an unrecognized `schema_version` rather than misinterpreting its fields; there is
no silent forward- or backward-compatible mutation. Back-compat is not owed indefinitely — a version
bump may drop support for older serialized worlds — but it must never happen implicitly inside an
unrelated feature change.

**The gate:** no transform (a generation pass, an extractor, a renderer) may reach into
`monomyth-core` and widen a type to fit its own convenience. A shape genuinely missing from the
contract is a proposal against this ADR — reviewed, versioned, and landed as a `monomyth-core` change
in its own commit — never an incidental edit riding on a transform's feature branch.

### Consequences

- Good, because extraction and generation share one round-trippable fact, making "extract then
  regenerate" and "generate then re-extract" meaningful operations to test against each other.
- Good, because the contract's purity (no IO/async/LLM/config/genre) survives the addition of a whole
  new transform direction, not just new renderers.
- Good, because a version bump is a visible, reviewable event instead of quiet schema drift hidden in
  a feature commit.
- Bad, because a genuinely-needed new shape must go through the gate even under deadline pressure;
  mitigated by keeping the gate itself lightweight (one constant bump + a documented reason).

### Confirmation

Seed-42 snapshot tests assert byte-identical serialized `World` output across unrelated changes (a
regression here means something silently touched the contract). Any PR that changes a
`monomyth-core` public type's serialized shape must bump `SCHEMA_VERSION` and add a one-line reason at
the constant — checked in review, not currently automated. `World::from_json_checked` rejects
mismatched `schema_version` at the load boundary (existing behavior, ADR-0001).

## Pros and Cons of the Options

### Freeze `monomyth-core` as the single pivot

- Good, because it is the only option under which extraction and generation are literally inverse
  transforms, which is the property that makes round-tripping testable.
- Good, because it reuses the `Content` seam instead of inventing an extraction-specific one.
- Neutral, because it puts real pressure on extraction (ADR-0018) to fit an existing shape rather than
  design its own — intentional, and the point of Phase 3's "contract-fit" slice.

### Extraction-specific intermediate model

- Good, because extraction could move fast without touching the shared contract.
- Bad, because a converter step becomes a second source of truth, and round-tripping through two
  models is not a meaningful test of either transform.

### Informal, ad hoc contract mutation

- Bad, because it is exactly the drift this ADR exists to prevent — the contract would accrue
  transform-specific fields with no versioning discipline, breaking snapshot tests silently.

## More Information

Extends ADR-0002 (hybrid generation — the `Content` seam this ADR generalizes to extraction) and
ADR-0003 (the slotmap graph model — the prior `SCHEMA_VERSION` bump this ADR turns into a standing
rule). Instantiates the P-CONTRACT plane of ADR-0013. Scope note: this ADR covers contract-purity
invariants, the versioning rule, and the "no silent contract mutation" gate — the extractor's internal
design (how text becomes `NarrativeEdit`s) is ADR-0018.
