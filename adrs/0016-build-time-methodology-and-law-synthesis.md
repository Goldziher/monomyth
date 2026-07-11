---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# map→synthesize→configure build-time methodology + reference-ingest law synthesis

## Context and Problem Statement

ADR-0004 distilled comparative-mythology scholarship into `artifacts/frameworks/*.json` by hand, once.
That was an instance of a repeatable pattern, not a one-off: monomyth needs to keep turning grounded
source material into ship-safe structural vocabulary — next, myth-theory beyond Campbell/Propp/Polti
(Lévi-Strauss, Dumézil, Witzel, Doty, Eliade, d'Huy), most of which sits in the `reference` namespace
and is copyrighted or NonCommercial. How do we formalize that pipeline, and how do we ingest and use
reference-only source material without ever letting it — or its prose — reach shippable output?

## Decision Drivers

- ADR-0004's hand-encoding was informal; a second, larger wave of source material (the queued
  reference-ingest work, "task I") needs a named, repeatable process rather than another one-off.
- The commercial licensing invariant (ADR-0005): `reference`/copyrighted/NonCommercial material may
  inform generation but must never be surfaced verbatim in shipped output.
- `Tier::System` (`crates/monomyth-knowledge/src/ledger.rs`) already exists precisely for
  "uncopyrightable idea/taxonomy, independently encoded, no prose reproduced" — the target shape for
  anything synthesized from reference sources.
- Reference ingest does not exist today: `Knowledge::ingest` accepts only ship-safe input; a reference
  collection and a synthesis step downstream of it are both new.

## Considered Options

- Name the build-time pipeline explicitly (`map → synthesize → configure`) as a plane with its own
  licensing gate: reference sources ingest into a reference-only collection, get synthesized by a
  human-reviewed step into `Tier::System` artifacts, which then feed run-time config/vocabulary.
- Continue hand-encoding each new framework ad hoc, as ADR-0004 did, with no named process or reusable
  reference-ingest path.
- Skip reference material entirely and derive all future structure only from `ship`-namespace sources.

## Decision Outcome

Chosen option: "name the build-time pipeline, with a licensing gate around reference synthesis". This
is the same pattern ADR-0004 already instantiated, made explicit and repeatable:

1. **Map** — pull concepts from grounded sources via `monomyth-knowledge`/xberg RAG, including
   `reference`-namespace myth-theory (Lévi-Strauss's binary oppositions, Dumézil's trifunctional
   hypothesis, Witzel's Laurasian/Gondwanan mythology, Doty's dimensions of myth, Eliade, d'Huy's
   phylogenetic reconstructions).
2. **Synthesize** — a human-reviewed step turns mapped concepts into `Tier::System` artifacts: the
   *idea or taxonomy* (e.g. "myths mediate a binary opposition," "a trifunctional caste maps to
   faction roles"), independently encoded, reproducing **no prose** from the source. These land beside
   `artifacts/frameworks/*.json`, validated the same way (ADR-0004's parity tests).
3. **Configure** — synthesized artifacts feed run-time config/vocabulary that `monomyth-gen` and later
   `monomyth-genre` read (through ADR-0015's resolver where the value is a tunable, or through
   `monomyth-frameworks`-style typed accessors where it is fixed vocabulary).

This is where the queued reference-ingest work ("task I") lands: add a *reference*-collection ingest
path (today `Knowledge::ingest` is ship-only), populate it with the curated myth-theory sources above,
retrieve via a reference-only query path, and synthesize the artifacts named above. **The licensing
gate, concretely:**

- Reference ingest writes **only** to the reference collection — never ship.
- Synthesized artifacts are stamped `Tier::System` — idea/taxonomy only.
- Every synthesis is **human-reviewed before commit** — no automated prose extraction ships unreviewed.
- The existing three-layer enforcement (ingest guard, retrieval filter, CI ledger check — ADR-0005)
  extends to cover the new reference sources; it is not bypassed or duplicated.

### Consequences

- Good, because future framework/law expansion follows one named process instead of each wave
  reinventing how source material becomes shippable structure.
- Good, because reference material — including in-copyright myth theory — can inform generation depth
  without ever risking verbatim surfacing, by construction (`Tier::System`, human review, namespace
  isolation).
- Good, because the synthesis step's output format (JSON artifacts validated like
  `artifacts/frameworks/*.json`) reuses machinery that already exists, rather than inventing a new
  artifact kind.
- Bad, because human review is a manual gate that does not scale to a large corpus of reference
  sources quickly; accepted because the alternative (automated synthesis with no review) is exactly
  the licensing risk ADR-0005 exists to prevent.

### Confirmation

Reference ingest writes exclusively to the reference collection — a test asserts a reference-tagged
document is unreachable through the ship-only retrieval path (`KnowledgeQuery::surfaceable`/
`is_surfaceable`). Synthesized artifacts carry `Tier::System` and pass the existing ledger-invariant
tests (no non-`ship` tier in the `ship` namespace). Each synthesized artifact has a recorded human
reviewer before it is committed — enforced by process/code review, not currently automated.

## Pros and Cons of the Options

### Named `map → synthesize → configure` pipeline with a licensing gate

- Good, because it turns a one-off (ADR-0004) into infrastructure the roadmap can lean on repeatedly
  (myth-theory now, other concept domains later).
- Good, because the licensing gate is stated once, at the pipeline level, instead of re-argued per
  source.
- Neutral, because it requires a new reference-collection ingest path that does not exist yet —
  scoped, bounded work, not speculative infrastructure.

### Continue ad hoc hand-encoding

- Bad, because it does not scale past the single framework wave ADR-0004 already did, and gives no
  structural answer to "how do we safely use reference material" for the queued myth-theory sources.

### Skip reference material entirely

- Bad, because it discards the scholarship (Lévi-Strauss, Dumézil, Witzel, Doty, Eliade, d'Huy) most
  likely to deepen structural grounding beyond Campbell/Propp/Polti, for a licensing risk that is
  already solved structurally by the ship/reference namespace split (ADR-0005).

## More Information

Extends ADR-0004 (frameworks as the first instance of this pattern) and ADR-0005 (reference informs,
never surfaces verbatim — this ADR's licensing gate is ADR-0005 applied to a new source category).
Builds on ADR-0010 (the acquisition pipeline this reference-ingest path reuses). The companion
walkthrough lives in `docs/methodology.md`. Scope note: this ADR covers the build-time authoring
pipeline and the reference-to-ship-safe-synthesis licensing gate — the run-time config *mechanism*
that consumes synthesized vocabulary is ADR-0015, not this ADR.
