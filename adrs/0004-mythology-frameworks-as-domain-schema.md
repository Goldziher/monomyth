---
status: accepted
date: 2026-07-06
decision-makers: Na'aman Hirschfeld
---

# Comparative-mythology frameworks as the domain schema

## Context and Problem Statement

The engine's story vocabulary — stages, functions, roles, situations, motifs — could be invented
ad hoc, but that risks a shallow, arbitrary model. monomyth's thesis is that adventures have deep
structure. Where does the domain schema come from?

## Decision Drivers

- Ground the model in established scholarship, not invention.
- A structure rich enough to drive procedural pacing and constrain LLM prompts.
- Language-neutral artifacts the Rust engine can consume directly.

## Considered Options

- Distill established analytic frameworks into validated JSON artifacts.
- Invent a bespoke schema from intuition.
- Learn structure statistically from a corpus with no explicit schema.

## Decision Outcome

Chosen option: "distill established frameworks". `artifacts/frameworks/*.json` encode a **3-tier plot
model** (Campbell monomyth → Propp functions + Polti situations → Thompson motifs) plus a
**character model** (Propp role × Greimas actant × archetype), bound by crosswalks
(arc, plot, character). A validator enforces canonical counts, crosswalk referential integrity, and
the license invariant.

### Consequences

- Good, because the schema is principled, documented, and immediately populated.
- Good, because artifacts are language-neutral JSON — the Rust `story` module reads them as-is.
- Bad, because some frameworks are copyrighted; we encode only the uncopyrightable systems/labels
  (the `system` tier) and reproduce no prose. See ADR-0005.

### Confirmation

`corpus/frameworks/validate.py` passes: canonical counts (Propp 31, Polti 36, Campbell 17, …),
crosswalk `reference_map` integrity, and the ledger invariant.

## More Information

ATU tale-types and the fine-grained Thompson motif table come from the CC-BY-SA `trilogy` dataset at
fetch time, not hand-encoding.
