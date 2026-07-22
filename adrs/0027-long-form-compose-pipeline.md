---
status: accepted
date: 2026-07-22
decision-makers: Na'aman Hirschfeld
---

# Long-form adventure prose: a staged compose pipeline downstream of the world model

## Context and Problem Statement

monomyth today produces a procedural `World` plus per-slot content-fill — each content-bearing entity
carries one `Content` slot the LLM turns from `Empty` into `Filled` (ADR-0002's procedural/content
split). That yields captioned structure, not a readable adventure. The product thesis needs *long-form
prose*: multi-paragraph narration that walks the hero's journey the World already encodes. How do we
generate that prose without (a) letting the LLM invent structure the procedural half owns, (b)
perturbing the determinism goldens and content-fill cassettes (ADR-0002), or (c) leaking rendering
concerns into the render-agnostic `monomyth-core` contract?

## Decision Drivers

- **Structure stays procedural.** The hero's-journey spine, its stages, and the cast are a
  deterministic function of the seed. Long-form generation may *narrate* that spine but must never
  invent or reorder it — the same invariant per-slot content-fill already honors.
- **Determinism discipline is non-negotiable.** The gen golden (`0x65f6_6a4b_a541_5419`) and the eval
  golden (`0xe852_73b6_ce90_fe55`) must not drift, and the LLM half must be replayable offline in CI.
- **Reuse, don't reinvent.** WS-B shipped a prose scorer (`score_semantic`) and WS-C shipped
  reference-path grounding; the long-form feedback loop should consume those, not grow a parallel
  apparatus. The `monomyth-llm` `StructuredBackend` seam and its `Recording`/`Replay` cassette
  backends already make LLM calls testable.
- **`monomyth-core` stays render-agnostic.** No colors, glyphs, coordinates — and, by the same logic,
  no assembled long-form document — leak into the shared `World` contract.

## Considered Options

- **Extend per-slot content-fill to long-form.** Make the content pass emit paragraphs instead of
  phrases into the existing `Content` slots.
- **Generate prose inside the `World` model.** Add a long-form field to `World` that the content pass
  fills.
- **A new downstream `monomyth-compose` crate.** A staged pipeline that takes a finished `World`
  (read-only) and emits a long-form document as a sidecar artifact, distinct from content-fill.

## Decision Outcome

Chosen option: **a new downstream `monomyth-compose` crate**, because long-form generation is a
genuinely different program from per-slot fill — it threads prior text across sections, retrieves
per-section grounding, and runs a score-driven revise loop — and folding that into the content pass
would entangle two concerns and put the content-fill cassette at risk. Keeping it a separate,
late-stage crate that only *reads* the `World` preserves the core contract and lets the pixel frontend
remain a drop-in.

**Shape.** `monomyth-compose` runs a staged, checkpoint-able pipeline over a read-only `World`:

- **Plan** — derive an `Outline` from `world.story.structure.spine()`: one `OutlineSection` per
  narrative node, carrying the node id, its primary `MonomythStage`, and the node's synopsis hint.
  This phase is **pure and deterministic** — the outline is a function of the World, so the LLM later
  narrates a structure it cannot alter. No LLM, no IO.
- **Draft** — for each section, a multi-turn generation primitive
  (`generate_long_form(instruction, grounding, prior) -> {text, is_complete}`, looped up to N turns)
  produces prose, threading already-generated text for coherence and per-section reference grounding
  (`Knowledge::retrieve` on the reference path) for on-topic detail.
- **Revise** — score a drafted section with the WS-B semantic scorer (embed the draft via
  `Knowledge::embed_texts`, `score_semantic` it against its grounding embeddings), derive improvement
  instructions from the weak axis, regenerate, and converge to a threshold or emit a missing-info note.
  This reuses the synthesis judge-loop pattern (ADR-0024); no new judge apparatus.
- **Assemble** — stitch the revised sections into the final `LongFormDoc`.

**Output is a sidecar artifact.** Compose returns a `LongFormDoc`, not a mutation of `World`. The
World's `Content` synopsis slots remain the structural seam; the assembled prose is a distinct
downstream product, so `monomyth-core` gains neither a long-form field nor a dependency.

**Determinism.** Compose never touches the procedural RNG (ADR-0002) — it consumes an
already-generated World, so the gen golden is untouched by construction. Its LLM phases are quarantined
behind the `StructuredBackend` seam and replayed in CI from a committed cassette (the gen
`live_fill`/`replay_fill` split, keyed by FNV-1a(schema_name+prompt)); the live recording path is
`#[ignore]`. The eval golden is unaffected because `report_fingerprint` never observes semantic data
(ADR-0026 / WS-B).

**Prompt roles (folds in WS-E / #42).** The pipeline introduces distinct prompt roles — outliner,
section-writer, reviser, shortener — each earning a named template and a stable `prompt_identifier`
for tracing and cassette keys. This is where prompt templating earns its keep, driven by real
multiplicity rather than added speculatively.

Delivered in slices:

- **Slice 1 — crate skeleton + Plan phase** (this change): the `monomyth-compose` crate,
  `Outline`/`OutlineSection`/`ComposeError` types, and `plan(&World) -> Result<Outline, ComposeError>`
  deriving the outline from the spine. Pure and deterministic — no LLM, no IO — so it de-risks the
  crate seam without touching any cassette or golden. Snapshot/property tests pin the outline against a
  seeded world.
- **Slice 2 — Draft + the multi-turn primitive** (realized): `generate_long_form`
  (`monomyth_compose::generate`) over the `StructuredBackend` seam, per-section grounding, and the
  `record`/`replay` cassette pair.
- **Slice 3 — Revise** (realized): the semantic-scored feedback loop (`monomyth_compose::revise`),
  driving `draft_and_revise_section` to convergence or a missing-info note.
- **Slice 4 — Assemble + `compose()` end-to-end + CLI wiring** (realized): `assemble` stitches
  revised sections into a `LongFormDoc` (`monomyth_compose::assemble`); `compose()`
  (`monomyth_compose::draft`) runs plan → draft/revise → assemble end-to-end; `monomyth-cli`'s
  `compose` subcommand (`crates/monomyth-cli/src/compose.rs`) wires it into the binary.

### Consequences

- Good, because the procedural/content split (ADR-0002) is preserved verbatim: compose reads a frozen
  World and cannot perturb the gen golden, so long-form generation adds no determinism risk to the
  structural half.
- Good, because it reuses shipped machinery — the WS-B scorer, WS-C grounding, the `StructuredBackend`
  cassette seam — rather than duplicating a judge or an embedder.
- Good, because `monomyth-core` stays render-agnostic: the long-form document is a sidecar, so the
  pixel frontend and the world contract are untouched.
- Bad, because the score-driven revise loop is a new source of LLM cost and latency per section; the
  multi-turn continuation must be bounded (max turns, max length) to stay affordable.
- Bad, because compose grows a dependency fan-in (core, knowledge, llm, eval, config) — it is the
  capstone crate and by design the widest consumer, so its build is the heaviest in the workspace.
- Neutral, because Plan lands first as a pure function with no LLM; the genuinely risky LLM phases
  arrive in later, individually-cassetted slices rather than all at once.

### Confirmation

Slice 1: `cargo test -p monomyth-compose` pins the Plan phase deterministically (a seeded world yields
a fixed outline — one section per spine node, stages in spine order); `cargo test --workspace` plus
both determinism goldens (gen `0x65f6_6a4b_a541_5419`, eval `0xe852_73b6_ce90_fe55`) confirm the new
crate perturbs nothing.

Slices 2-4 (realized): `crates/monomyth-compose/tests/compose_replay.rs` is the CI-hard offline test,
loading the committed `tests/cassettes/compose_seed42_gemini.json` via `ReplayBackend` end-to-end
through `compose()`; `crates/monomyth-compose/tests/compose_live.rs` is the `#[ignore]`d live
recording path, mirroring `monomyth-gen/tests/{live,replay}_fill.rs`. `monomyth-cli`'s `compose`
subcommand exercises the same pipeline from the binary. `cargo test --workspace` and both determinism
goldens remain green — compose's LLM phases stay quarantined behind the `StructuredBackend` seam and
never touch the procedural RNG or the eval fingerprint.

## Pros and Cons of the Options

### Extend per-slot content-fill to long-form

- Bad, because it entangles two distinct programs (phrase-fill vs. multi-turn narration) and puts the
  content-fill cassette at risk every time the long-form logic changes.
- Bad, because per-slot fill has no notion of prior-text threading or a revise loop; retrofitting them
  distorts a deliberately simple pass.

### Generate prose inside the `World` model

- Bad, because it leaks a rendered artifact into the render-agnostic core contract, breaking the
  drop-in-frontend property.
- Neutral, because it would centralize output, but the World is the wrong home for a late-stage,
  optional product.

### A new downstream `monomyth-compose` crate

- Good, because the risky, iterative long-form logic lives in its own crate with its own cassettes,
  isolated from the frozen structural half.
- Good, because it reads the World read-only, so the core contract and the gen golden are safe by
  construction.
- Neutral, because it is the widest consumer in the graph (core + knowledge + llm + eval + config),
  accepted as the cost of being the capstone.

## More Information

Builds on ADR-0002 (the procedural/content determinism split this pipeline sits downstream of and
never violates), ADR-0024 (the judge-gated feedback loop the revise phase reuses), ADR-0025/ADR-0026
(the reference retrieval + enrichment that grounds each section), and WS-B's semantic scorer
(`score_semantic`, the prose axis the revise loop consumes). Prompt templating (#42 / WS-E) is folded
in as the pipeline's prompt layer rather than shipped standalone. The permanent config home for
compose knobs (max turns, length bounds, per-role models) is the `[compose]` section tracked against
ADR-0015; slice 1 needs none. Revisit section granularity (per-stage vs. finer) and whether the
`LongFormDoc` should ever be persisted into the World once the Draft/Revise slices reveal real usage.
