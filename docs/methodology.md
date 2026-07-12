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
| Map | `Knowledge::ingest_reference` — the inverse gate of `Knowledge::ingest`, admitting **only** `reference`-namespace sources into the reference collection — pulls in curated myth-theory sources: Lévi-Strauss (structural binary oppositions), Dumézil (trifunctional hypothesis), Witzel (Laurasian/Gondwanan mythology), Doty (dimensions of myth), Eliade, d'Huy (phylogenetic reconstructions). Ingested at the CLI via `monomyth ingest --reference` (which stamps ADR-0005 provenance) and retrieved via `KnowledgeQuery::reference` (never surfaceable). |
| Synthesize | The `monomyth-synthesis` crate **drafts** a `Tier::System` "law" candidate — the idea or taxonomy, independently encoded, **no prose reproduced** — and a human **reviews and promotes** it. Drafting is a judge-gated feedback loop (ADR-0024); the candidate is written pre-review (`reviewed_by` empty, so `load_law` refuses it) into a gitignored `synthesis/candidates/` and is only shippable once a human fills the reviewer and moves it into `artifacts/laws/`. |
| Configure | Promoted artifacts land in `artifacts/laws/` beside `artifacts/frameworks/*.json`, validated the same way (`load_law`: tier `system`, namespace `ship`, contiguous ids, non-empty reviewer). They feed `monomyth-gen` and later `monomyth-genre` — as tunable config through `monomyth-config`'s resolver where the value is a knob, or as typed vocabulary where it is fixed structure. |

This is where roadmap item I ("land reference-ingest law synthesis," see
[`roadmap.md`](./roadmap.md#phase-1)) lands.

### The synthesis pipeline (`monomyth-synthesis`)

The **synthesize** step is not a single LLM call — a single distillation pass under-collects (a source
supporting a dozen macro-phases flattens into three or four coarse buckets). `draft_law` instead runs
a **retrieve → distill → judge → refine → gate → stamp** loop:

1. **Retrieve** multi-query reference grounding (ADR-0025): the seed query, optional
   framework-vocabulary coverage queries (one per taxonomy stage — e.g. Campbell's seventeen —
   seeded up front so the grounding spans the whole arc by construction), plus, mid-loop, the phases
   the judge names missing, all unioned and deduplicated.
2. **Distill** an initial candidate with an exhaustiveness-demanding prompt that also instructs
   *honesty over invention*: rather than fabricate a structurally-expected phase the grounding does
   not support, the model may include it and mark it `derivable: false` (an explicit abstention).
3. **Judge** the candidate with a second LLM call against weighted criteria (exhaustiveness, source
   grounding, abstraction, ordering, tier fit, and honest abstention — a marked gap is rewarded over
   a fabricated phase), yielding a score and a list of missing phases. Alongside it, a **deterministic
   advisory pre-score** (phase coverage vs the targeted framework, ordering monotonicity, grounding
   overlap) is computed with no LLM call — recorded for the reviewer as an independent second opinion,
   never mixed into the judge's score and never used to accept or reject.
4. **Refine** while the weighted score is below a rising bar — retrieve grounding for the missing
   phases and re-draft — keeping the best-scoring candidate across iterations (ADR-0024).
5. **Gate** the best candidate through the machine-checked anti-leak shingle check (a fourth
   enforcement layer atop ADR-0005's three — see [Licensing gate](#licensing-gate)); a verbatim
   overlap refuses the candidate outright.
6. **Stamp** a pre-review `LawArtifact` and write it plus a `.context.json` sidecar carrying the
   grounding, the judge trail (`final_score`, `iterations`, the full verdict), the advisory pre-score,
   and the honestly-abstained phase names for the reviewer.

The CLI entry point is `monomyth synthesize law --law <id> --domain <d> --query <q>`, with an opt-in
`--coverage-framework <name>` (e.g. `campbell`) that seeds the coverage queries. It refuses any `--out`
under `artifacts/` and prints a REVIEW-REQUIRED banner — now including a `Judge score: N/100 after K
iteration(s)` line — with the promotion steps. The judge raises the floor on completeness; **human
review remains the ceiling**, and the empty `reviewed_by` is what makes that non-optional.

## Inspect-download mode

Some declared sources are bulk dataset wrappers (`gutenberg_english`, `pg19`) whose per-item
public-domain status is **not verified** — the wrapper's own license (MIT / Apache-2.0) covers dataset
packaging, not each contained work. These are declared `reference`/`Tier::Reference` in
`corpus/manifest.json`, never `ship`, until an item is individually verified and promoted.

`monomyth corpus inspect` downloads reference/unverified sources into the `reference/` blob prefix
(ADR-0012) for human license and provenance review, exactly like `corpus build` fetches ship sources
into `ship/` — but it **never ingests** into the ship collection (ADR-0005). The guarantee is
structural, not a filter that could be bypassed: `inspect_one_source` (the per-source worker behind
`inspect_corpus`) takes no `Knowledge` parameter and has no code path to `Knowledge::ingest` anywhere
in its body. It is reinforced by `classify_entry`, which filters ship-namespace sources out of inspect
mode and reference-namespace sources out of build mode — so a source can never be dispatched to the
wrong pipeline by mistake.

> **Migration note (ship → reference re-namespace).** `gutenberg_english` and `pg19` were originally
> declared `ship`; they are now `reference`. Retrieval validates the namespace tag *stored on each
> document at ingest time*, not a live re-check against the current ledger, so a local `monomyth.db`
> built **before** the re-namespace may still hold chunks from these sources tagged `ship` — and those
> would pass the ship filter. Any store built before the re-namespace must have its `ship` collection
> dropped and rebuilt (`corpus build`) so the stored tags match the current ledger. A fresh store is
> unaffected.

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
- A **fourth, synthesis-specific layer**: the anti-leak shingle gate (`verify_no_verbatim`) refuses a
  drafted candidate that shares any 8-word run with a reference passage — a machine-checked backstop
  against a distillation model reproducing source wording. It catches verbatim spans, not semantic
  copying; the real ceiling remains `Tier::System` plus human review.

See [`crates/monomyth-knowledge/src/ledger.rs`](../crates/monomyth-knowledge/src/ledger.rs) for the
`Namespace`/`Tier` types that carry this invariant, and ADR-0005/ADR-0016 for the full reasoning.
