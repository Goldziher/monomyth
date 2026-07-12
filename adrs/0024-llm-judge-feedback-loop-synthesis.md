---
status: accepted
date: 2026-07-12
decision-makers: Na'aman Hirschfeld
---

# LLM-as-judge feedback loop for reference→law synthesis

## Context and Problem Statement

ADR-0016 named the `map → synthesize → configure` pipeline, whose **synthesize** step distils
reference-namespace priors into a ship-safe `Tier::System` structural law. The first implementation
was a single LLM distillation pass. In practice one pass **under-collects**: grounding that supports
a dozen distinct macro-phases collapses into three or four coarse buckets, and whole acts go missing
(the first real run produced a law titled *"The Journey of Return"* that contained no return phase).
Hand-tuning the distillation prompt until it happens to be exhaustive is brittle and does not
generalise across sources. How do we get exhaustive, grounded synthesis reliably — without a human
hand-iterating every draft, and without weakening the licensing gate?

## Decision Drivers

- **Under-collection is the dominant failure mode**, not hallucination or leakage — the anti-leak
  gate (ADR-0005) already handles verbatim reproduction; completeness is the open problem.
- The synthesised output must stay `Tier::System` + anti-leak-gated + human-reviewed (ADR-0016) —
  any iteration mechanism composes with those gates, it does not replace them.
- It must be **offline-testable and deterministic** under the crate's cassette / canned-backend
  discipline; the procedural half's determinism (the golden FNV hash) must not be touched.
- A repeatable quality mechanism beats per-source prompt-babysitting.

## Considered Options

- **Single pass with a more exhaustive prompt** — keep one LLM call, just instruct it harder.
- **An LLM-as-judge feedback loop** — score each candidate against weighted criteria, and while it is
  below a rising passing bar, retrieve targeted grounding for the phases the judge names missing and
  re-draft against the critique; keep the best-scoring candidate.
- **Leave iteration entirely to the human reviewer** — ship the coarse first draft and let review fix
  it.

## Decision Outcome

Chosen option: "an LLM-as-judge feedback loop", modelled on the grantflow RAG service's
`evaluate_with_feedback_loop` (a rising passing bar, best-so-far tracking, per-criterion weighted
scoring, and explicit "missing" signals). The loop lives in `monomyth-synthesis::draft_law`:

1. **Draft** — distil an initial candidate from multi-query grounding (ADR-0025). The distillation
   prompt demands macro-tier exhaustiveness across the whole arc, including the return/restoration.
2. **Judge** — a second LLM call returns a `JudgeVerdict { criteria: [{name, score 0-100,
   rationale}], missing_phases, instructions }`. `weighted_score` folds the per-criterion scores by
   fixed weights (Exhaustiveness 1.0, Source grounding 0.9, Abstraction 0.8, Ordering & non-overlap
   0.7, Tier fit 0.6).
3. **Refine** — if the score clears a rising bar (default 70.0, +5.0/iteration) the candidate is
   accepted; otherwise the judge's `missing_phases` become targeted reference queries, their results
   are merged into the grounding, and the model re-drafts a complete candidate against the critique.
4. **Keep best** — over at most `max_iterations` (default 3) the highest-scoring candidate wins.
5. **Gate + stamp** — the best candidate runs the **anti-leak hard gate** (unchanged from ADR-0016)
   and is stamped into a pre-review `LawArtifact` with an empty `reviewed_by`. The judge trail
   (`final_score`, `iterations`, `verdict`) travels on `DraftedLaw` for the human reviewer.

The judge produces a **soft** score that gates iteration; the anti-leak check remains a **hard** gate
that no score can override. The model's authority is still only the idea/taxonomy.

### Consequences

- Good, because exhaustiveness improves without per-source prompt-babysitting: the judge names the
  specific gaps, and those names drive the next retrieval — coverage and critique reinforce each
  other.
- Good, because it composes with, rather than bypasses, the existing gates: `Tier::System`, the
  anti-leak shingle check, and mandatory human review are all still in force.
- Good, because it is offline-testable: a schema-dispatching canned backend returns scripted
  `CandidateLaw` / `JudgeVerdict` responses, so accept-first-pass, refine-then-accept,
  keep-best-when-never-passing, and anti-leak-still-hard-gates are all deterministic tests.
- Bad, because a draft now costs several LLM calls (draft + judge + refine…); bounded by
  `max_iterations`, and acceptable because synthesis is a build-time, human-reviewed step, not a
  hot path.
- Bad, because the judge is itself a non-deterministic LLM; mitigated by best-of-N selection and by
  keeping human review as the real quality ceiling — the judge raises the floor, it does not certify
  the result.

### Confirmation

`cargo test -p monomyth-synthesis`: `weighted_score` is unit-tested with exact arithmetic; the loop
is covered by canned-backend tests for each control-flow branch (accept on first pass, refine then
accept, keep best when the bar is never met) and by a test proving the anti-leak gate still refuses a
verbatim-overlapping candidate even when the judge scored it highly. The judge trail is recorded on
`DraftedLaw` and surfaced to the reviewer.

## More Information

Extends ADR-0016 (this is the **synthesize** step made iterative) and composes with ADR-0005 (the
anti-leak gate is untouched). Retrieval for the loop is ADR-0025. Influenced by the grantflow RAG
service's evaluation subsystem (`utils/evaluation/feedback_loop.py`, `evaluation_criteria.py`):
the rising-bar loop, per-criterion weighted scoring, and best-so-far tracking are adopted; grant-
domain heuristics and the grant-application staging are not. Deferred adoptions captured as roadmap
follow-ups: a deterministic pre-score fast-path before the LLM judge, "could-not-derive" markers
(grantflow's `MISSING INFORMATION` pattern, rewarded not penalised), judge-result caching, and a
synthesis quality-baseline benchmark under ADR-0023.
