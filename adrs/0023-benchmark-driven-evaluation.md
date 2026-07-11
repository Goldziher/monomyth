---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# Benchmark-driven evaluation + ground-truth corpus

## Context and Problem Statement

The contract's bidirectional claim — extraction (text → `World`, ADR-0018) and generation
(`World` → text, ADR-0002) are inverse transforms — and the scored-attribute schema (ADR-0022) are
both unfalsifiable without ground truth: nothing today checks an extracted `World` against what a
source text actually says, or scores a classifier's output against a known-correct distribution.
This works only when we establish ground truths against certain benchmark works. How do we get
quantitative, regression-testable proof on well-studied classical texts, without smuggling
copyrighted scholarship into shippable output or inventing a second corpus pipeline?

## Decision Drivers

- The extraction⇄generation round trip and the scored-attribute schema are claims about accuracy;
  claims need a measurement, not just passing `World::validate`.
- Ground-truth fixtures must respect the ship/reference invariant (ADR-0005) — hand-encoding from
  published scholarship must never bake reference prose into a shippable fixture.
- The eval harness must be deterministic and regression-testable like the rest of the contract
  (ADR-0014's FNV-golden precedent), not a fuzzy LLM-judged score that drifts between runs.
- Text↔scored-structure pairs from PD works are valuable fine-tuning data later; the harness should
  produce that as a byproduct, not a separate effort.
- The Odyssey macro-axis PoC should not sit fully blocked on ADR-0022 landing — `NarrativeNode.stage`
  is already schema v2, so a thin vertical slice can start in parallel.

## Considered Options

- Ground-truth fixtures as full serialized `World`s (empty content, PD-gated `source_id`) + a
  deterministic `monomyth-eval` scoring crate + a hermetic RAG-softmax extraction PoC, one deep
  vertical slice (Odyssey, Campbell macro axis) first.
- LLM-as-judge scoring against source text directly, no fixtures.
- Defer evaluation entirely until extraction and the scored-attribute schema are both fully built.

## Decision Outcome

Chosen option: "ground-truth fixtures + deterministic eval crate + hermetic RAG-softmax PoC, one
deep vertical slice first", because it is the only option that makes the bidirectional claim
falsifiable now, reuses the validated write surface instead of inventing a parallel one, and keeps
the licensing invariant structural rather than a matter of author discipline.

**Ground-truth fixtures.** A benchmark work is a full serialized `monomyth-core` `World`,
hand-encoded from published scholarship and committed under `artifacts/benchmarks/`
(poly-excluded, same as the other framework artifacts). Every `Content` slot is `Content::Empty` —
no prose is ever baked into a fixture. The actual PD text lives in the corpus and is referenced by
`source_id`, which **must** resolve to a public-domain / CC0 / permissive ledger entry; a registry
test enforces this and fails the build if a fixture's `source_id` resolves to a `reference` /
copyright / NonCommercial tier (ADR-0005's invariant, extended to fixtures). Fixtures are authored
either through a reviewable `Vec<NarrativeEdit>` edit-script — the canonical path — applied via
`apply_edits`, or as hand-written JSON; both paths are gated by `World::validate` /
`from_json_checked` and pinned with an FNV golden hash, mirroring the existing snapshot-test
pattern (ADR-0014).

**Eval harness.** A new `crates/monomyth-eval` crate scores extraction/classification output
against a fixture, deterministically and with no LLM in the scoring path. Core types: `Dist<T>`
(a discrete distribution over an attribute's possible values) and `DistScore`. The scored-attribute
metric set: weighted **histogram-intersection** as the primary metric, **cross-entropy** for
training alignment, **top-1 accuracy**, and **Kendall-τ** for rank agreement. A **node-alignment
scorer** matches extracted narrative nodes to gold nodes before per-node metrics can be computed —
this is the one genuinely novel piece of the harness, and it must be unit-tested against
hand-crafted near-miss DAGs before any aggregate score built on top of it is trusted. Scoring output
is a `Report`, itself pinned with its own FNV golden.

**Extraction PoC.** A new `crates/monomyth-extract` crate implements the `Extractor`/`Classifier`
traits — seams declared in `monomyth-contracts` per ADR-0013/ADR-0018 — writing exclusively through
`NarrativeEdit` + `apply_edits`, grounded by `Knowledge::retrieve`. The thinnest first slice is the
**Odyssey, Campbell-macro axis**, via a **RAG-softmax classifier**: no LLM, fully hermetic, usable as
a CI baseline. It turns retrieval similarity scores into a permille `Dist<MonomythStage>` — a plain
softmax over `Knowledge::retrieve` scores, nothing learned, nothing external. An LLM-backed
extractor is a later increment; the hermetic baseline goes first specifically so CI has something to
measure against on every commit.

**CLI + fine-tune export.** A CLI `eval` subcommand runs a benchmark end-to-end (fixture in,
`Report` out). A fine-tune JSONL exporter produces text↔scored-structure pairs and refuses to export
any pair whose `source_id` does not resolve to `public_domain` / `cc0` / `permissive` — it never
exports reference text or scholarship prose, full stop.

**Parallelism note.** `NarrativeNode.stage` is already populated in schema v2, so the Odyssey macro
benchmark and the RAG-softmax slice can start in parallel with the ADR-0022 schema refactor; the PoC
is not fully blocked on the contract change landing first.

### Consequences

- Good, because the bidirectional claim and the scored-attribute schema get a falsifiable,
  quantitative measurement instead of resting on `World::validate` passing.
- Good, because the ground-truth corpus is PD-clean by construction (registry-enforced), so it
  doubles as fine-tuning data later with no separate licensing review.
- Good, because the harness reuses existing infrastructure end to end — `apply_edits`,
  `World::validate`, the FNV-golden pattern, `Knowledge::retrieve` — only the scorer and the
  extractor traits are green-field.
- Bad, because node-alignment correctness is the sharpest risk in the harness: a wrong alignment
  silently corrupts every downstream metric; mitigated by unit-testing it against hand-crafted
  near-miss DAGs before trusting any aggregate score built on it.
- Bad, because hand-encoding a fixture from scholarship is subjective; mitigated by per-weight
  justification and two independent annotators on the first fixture (the Odyssey).
- Bad, because fine-grained ATU tale-type scoring is not available until reference-ingest
  law-synthesis (ADR-0016) lands; accepted by scoring the top-level `AtuCategory` only until then.
- Neutral, because starting with one deep vertical slice (Odyssey macro) instead of many shallow
  ones means broader coverage is deferred — a deliberate trade favoring depth of proof over breadth.

### Confirmation

The registry test fails the build if any `artifacts/benchmarks/*` fixture's `source_id` resolves to
a non-`public_domain`/`cc0`/`permissive` ledger entry. Fixture FNV goldens and the `Report` FNV
golden are snapshot-tested like existing contract snapshots. The node-alignment scorer has unit
tests against hand-crafted near-miss DAGs, run before any test that depends on aggregate scores.
The fine-tune exporter has a test asserting it refuses a `reference`-tagged `source_id`.

## Pros and Cons of the Options

### Ground-truth fixtures + deterministic eval crate + hermetic RAG-softmax PoC

- Good, because every piece — fixture format, scoring, extraction seam — reuses an already-decided
  contract surface (ADR-0002, ADR-0014, ADR-0018) rather than inventing new machinery.
- Good, because the hermetic baseline (no LLM) is cheap to run in CI on every commit.
- Neutral, because it commits to building the node-alignment scorer now, before extraction has more
  than one axis — the sharpest engineering risk, taken on deliberately and up front.

### LLM-as-judge scoring against source text directly

- Bad, because it is non-deterministic between runs, which conflicts with the project's snapshot/
  golden-hash discipline (ADR-0014) and cannot be a CI regression gate.
- Bad, because it gives no reusable fine-tuning artifact — a judged score is not a text↔structure
  pair.

### Defer evaluation entirely

- Good, because it costs nothing today.
- Bad, because it leaves the extraction⇄generation claim and the scored-attribute schema
  unfalsifiable indefinitely, and the PD-clean fine-tuning corpus never gets built as a byproduct of
  ongoing work.

## More Information

Upholds ADR-0005 (the PD-source gate and no-verbatim-reference invariant — extended here to
ground-truth fixtures, which must be as clean as shippable corpus material). Reuses ADR-0014's
validate/`NarrativeEdit`/FNV-golden surface as the fixture-authoring and pinning mechanism. Measures
ADR-0018's extraction subsystem — `monomyth-extract`'s first real implementation is the RAG-softmax
PoC this ADR introduces. Scores the attributes introduced by ADR-0022; the Odyssey macro-axis slice
is deliberately sequenced to start before that schema work fully lands, since `NarrativeNode.stage`
is already schema v2.
