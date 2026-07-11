---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# Scored / weighted classification attributes for framework tags

## Context and Problem Statement

ADR-0004's framework tags (Propp functions, Polti situations, Campbell stages, Thompson motifs,
Booker plots, Propp roles, archetypes) are modeled in `monomyth-core` as single categorical values or
unweighted sets (`BTreeSet<ProppFunction>`, `Option<PoltiSituation>`, a bare `MonomythStage`, …).
Real scholarship classifies a narrative element as a *distribution* over categories with disagreement
— a beat might be 70% one Propp function, 30% another — and today's generation selects among
framework candidates *uniformly*, discarding how strongly a category applies. How does the contract
represent classification-with-uncertainty, and how does generation select rules by it?

## Decision Drivers

- Scholarship-faithful representation: annotators disagree and categories overlap; a bare label
  discards that signal.
- Weighted generation: rule selection should be able to favor a stronger-scored candidate over a
  weaker one, not draw uniformly.
- Determinism (ADR-0002, ADR-0003) and the contract's byte-stable serialization (ADR-0014) must
  survive the change — no floating point in the serialized shape.
- Forward compatibility with extraction (ADR-0018), which will want to emit distributions rather than
  single labels, and with a future eval (weighted-selection bias needs a statistical, not byte-exact,
  guard).

## Considered Options

- Generalize every categorical field into a uniform scored attribute: a label plus one-or-more
  weighted values, using an integer permille `Weight`.
- Keep single categorical values; bolt on an ad hoc confidence field per framework enum as needed.
- Use `f32`/`f64` weights directly on the existing set/option types.

## Decision Outcome

Chosen option: generalize every categorical framework-typed field into a uniform **scored
attribute** = a label plus one-or-more weighted values (a permille distribution over that
classification system's categories).

- `Weight` newtype = `u16` permille in `0..=1000`, `#[serde(transparent)]`, `Default` = FULL (1000).
  **No floating point anywhere in the contract** — weights are relative shares (need not sum to
  1000; normalized at draw time), so serialization stays bit-exact and determinism (seed→replay,
  snapshot stability) is preserved. This float-avoidance is the load-bearing reason for the `u16`
  choice over `f32`/`f64`.
- Two typed containers in `monomyth-core`: `ScoredSet<T>(BTreeMap<T, Weight>)` for zero-or-more
  axes; `ScoredOne<T> { primary: T, alternatives: BTreeMap<T, Weight> }` for exactly-one axes (the
  `primary` is the mandatory label; alternatives are scored competitors). Typed rather than a
  stringly-keyed bag, to keep `from_id` validation and `BTreeMap` canonical ordering. A
  `From<BTreeSet<T>>` bridge migrates v2 data to all-FULL weights.
- The six affected fields: `NarrativeNode.functions`/`.motifs` → `ScoredSet`; `NarrativeNode.situation`
  → `Option<ScoredOne>`; `NarrativeNode.stage` → `ScoredOne<MonomythStage>` (mandatory tag ⇒
  `.primary`); `Story.plot`, `Quest.situation`, `Entity.role`, `Entity.archetype` →
  `Option<ScoredOne>`. `NodeSpec` and the four `SetNode*` narrative-edit variants mirror these.
- Weighted rule selection: `draw_weighted_index(rng, &[Weight]) -> Option<usize>` computes an
  integer cumulative sum and takes one modulo — **exactly one RNG stream word consumed per draw** —
  so the per-pass sub-stream determinism discipline (ADR-0002) is intact. Crosswalk artifacts
  (`artifacts/frameworks/*.json`) gain parallel weight arrays; a missing array means all-FULL.
- This executes under ADR-0014's `SCHEMA_VERSION` gate: bump `2 → 3`. Because both the serialized
  shape and the draw modulus change, output cannot be byte-identical to v2. The durable determinism
  guard therefore shifts, for this phase, from a byte-identical golden to a **distributional
  chi-square test** ("uniform weights ⇒ statistically unbiased selection"); the golden FNV hash is
  re-pinned and reviewed as an eyeballed diff, per ADR-0014's confirmation practice.

### Consequences

- Good, because it represents scholarly disagreement and category overlap directly, instead of
  forcing a single best-guess label.
- Good, because generation can weight rule selection by scored strength, not draw uniformly.
- Good, because it sets up the benchmark eval's distribution metrics (ADR-0023, planned) and lets
  extraction (ADR-0018) emit distributions rather than collapse to one label.
- Bad, because it forces a wide but mechanical sweep of every `.stage`-reading call site to
  `.stage.primary`.
- Bad, because the golden FNV hash churns with this phase's schema bump, same cost as any
  ADR-0014 shape change.
- Bad, because annotation subjectivity is now encoded explicitly as permille splits, which is more
  honest but also a larger, more inspectable surface for reviewers to second-guess.

### Confirmation

`Weight` round-trips through serde as a bare integer (`#[serde(transparent)]`); `SCHEMA_VERSION`
is `3` with a documented reason at the constant (ADR-0014's rule). A distributional chi-square test
asserts uniform-weight `draw_weighted_index` selection is statistically unbiased across a fixed seed
sample. Crosswalk artifact validation still enforces canonical counts, referential integrity, and the
license invariant (ADR-0004) with the added weight arrays.

## Pros and Cons of the Options

### Uniform scored attribute (`Weight` + `ScoredSet`/`ScoredOne`)

- Good, because one pair of container types covers all seven affected framework systems instead of
  seven bespoke shapes.
- Good, because integer permille keeps the contract float-free and bit-exact, preserving ADR-0002's
  replay guarantee at the type level.
- Neutral, because `ScoredOne.primary` being mandatory means every exactly-one axis (stage, plot,
  role, archetype) keeps a non-optional fallback even when no alternatives are scored.

### Ad hoc confidence field per enum

- Bad, because it produces seven divergent shapes instead of one, with no shared draw or validation
  logic — the opposite of ADR-0004's "typed enums + crosswalk accessors" discipline.

### Floating-point weights

- Bad, because `f32`/`f64` break bit-exact serialization and reintroduce platform/rounding
  nondeterminism into a contract that ADR-0002/ADR-0003 depend on being exactly replayable.

## More Information

Amends ADR-0004 (framework tags become scored distributions instead of bare categorical values).
Executed under ADR-0014's `SCHEMA_VERSION` gate and determinism/confirmation policy. Relates to
ADR-0018 (extraction can emit distributions into these same containers) and ADR-0023 (planned; the
benchmark eval consumes weighted distributions as a metric).
