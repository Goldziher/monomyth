# Methodology: map → synthesize → configure

Companion to [ADR-0016](../adrs/0016-build-time-methodology-and-law-synthesis.md). This is the
repeatable procedure the **build-time plane** (P-KNOW + P-RULE, see
[`architecture.md`](./architecture.md)) uses to turn source material into vocabulary the run-time
transforms (generation, later extraction) consume.

## The procedure

```text
map/extract concept  →  synthesize into rule(s)  →  translate into configuration
```

1. **Map.** Pull concepts from grounded source material via `monomyth-knowledge`/xberg RAG. Sources
   may be `ship` (freely shippable) or `reference` (informs generation, never surfaced verbatim).
2. **Synthesize.** A human-reviewed step distills mapped concepts into a structured, language-neutral
   rule or taxonomy — the *idea*, not the source's prose. Output is a validated JSON artifact.
3. **Configure.** The synthesized artifact feeds run-time vocabulary or tunable config: fixed
   vocabulary through typed accessors (`monomyth-frameworks`-style), tunables through
   `monomyth-config`'s layered resolver (ADR-0015).

Step 2 is the licensing gate. What crosses from "informed by copyrighted scholarship" to "ship-safe
artifact" is the idea or taxonomy, independently encoded — never prose. See
[Licensing gate](#licensing-gate) below.

## Worked example 1: the mythology frameworks

The original, informal instance of this pattern (ADR-0004), predating its formalization in ADR-0016.

| Step | What happened |
|---|---|
| Map | Campbell's monomyth stages, Propp's 31 functions, Polti's 36 dramatic situations, Thompson's motif index, Greimas's actant model — established comparative-mythology and narratology scholarship. |
| Synthesize | Hand-encoded into a **3-tier plot model** (Campbell macro-arc → Propp functions + Polti situations → Thompson motifs) and a **character model** (Propp role × Greimas actant × archetype), bound by crosswalks (arc, plot, character). |
| Configure | Committed as `artifacts/frameworks/*.json`. `monomyth-frameworks` reads them as typed enums + crosswalk accessors; `monomyth-gen`'s passes and content prompts target the crosswalks. |

Validation: `monomyth-frameworks`'s parity tests enforce canonical counts (Propp 31, Polti 36,
Campbell 17, …), crosswalk referential integrity, and the ledger invariant — the same validation any
new framework artifact must pass.

Licensing note: some source frameworks are themselves copyrighted works. Only the uncopyrightable
system/label (the `system` tier — a list of stage names, a taxonomy) is encoded; no prose is
reproduced. ATU tale-types and the fine-grained Thompson motif table are sourced separately from the
CC-BY-SA `trilogy` dataset at fetch time, not hand-encoded, and are isolated in the `ShareAlike` tier
so their share-alike obligation cannot contaminate PD/CC0 material.

## Worked example 2: reference-ingest law synthesis

The formalized instance (ADR-0016), extending the pattern to a second wave of myth-theory scholarship
that sits predominantly in the `reference` namespace (copyrighted or NonCommercial).

| Step | What happens |
|---|---|
| Map | A *reference*-collection ingest path (new — today `Knowledge::ingest` is ship-only) pulls in curated myth-theory sources: Lévi-Strauss (structural binary oppositions), Dumézil (trifunctional hypothesis), Witzel (Laurasian/Gondwanan mythology), Doty (dimensions of myth), Eliade, d'Huy (phylogenetic reconstructions). Retrieved via a reference-only query path. |
| Synthesize | A human-reviewed step turns mapped concepts into `Tier::System` "law" artifacts — the idea or taxonomy, independently encoded, **no prose reproduced**: the Laurasian/Gondwanan two-arc structure, binary-opposition mediation, trifunctional faction casting, Doty's dimension checklist. |
| Configure | Synthesized artifacts land beside `artifacts/frameworks/*.json`, validated the same way. They feed `monomyth-gen` and later `monomyth-genre` — as tunable config through `monomyth-config`'s resolver where the value is a knob, or as typed vocabulary where it is fixed structure. |

This is where roadmap item I ("land reference-ingest law synthesis," see
[`roadmap.md`](./roadmap.md#phase-1)) lands.

## Frameworks vs. config: the boundary

Two different things both get called "configuration" informally. Keep them distinct:

| | Framework artifacts (`artifacts/frameworks/*.json`) | Config (`monomyth-config`) |
|---|---|---|
| What it is | Build-time ground-truth *vocabulary* | Run-time *tunable parameters* |
| Who conforms to whom | The code conforms to the artifact | Config tunes how a transform *uses* an artifact |
| Mutable at run-time? | No — immutable, versioned by commit | Yes — layered override, resolved per invocation |
| Example | Campbell's 17 stages, Propp's 31 functions | `fork_chance_permille`, `GROUNDING_TOP_K`, `MIN_ROOMS` |
| Who can change it | A reviewed commit to the artifact | An operator, a project file, a user override |

An operator must never be able to override Campbell's 17 stages through config. Config may *select or
weight* an artifact (e.g. "prefer this crosswalk branch more often"), never *edit* it. This boundary is
a hard rule in ADR-0013/ADR-0015, not a style preference.

## Licensing gate

Every synthesis step from `reference` material is gated, concretely:

- Reference ingest writes **only** to the reference collection — never `ship`.
- Synthesized artifacts are stamped `Tier::System` — idea/taxonomy only, no prose.
- Every synthesis is **human-reviewed before commit** — no automated prose extraction ships
  unreviewed.
- The existing three-layer enforcement (ingest-time refusal, retrieval-time
  `Filter::Eq("doc.metadata.namespace", "ship")`, CI ledger check — ADR-0005) extends to cover new
  reference sources; it is never bypassed or duplicated.

See [`crates/monomyth-knowledge/src/ledger.rs`](../crates/monomyth-knowledge/src/ledger.rs) for the
`Namespace`/`Tier` types that carry this invariant, and ADR-0005/ADR-0016 for the full reasoning.
