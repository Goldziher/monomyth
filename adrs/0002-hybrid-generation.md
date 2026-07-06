---
status: accepted
date: 2026-07-06
decision-makers: Na'aman Hirschfeld
---

# Hybrid generation (procedural + LLM)

## Context and Problem Statement

Adventures need both reproducible structure (maps, stats, pacing, quest skeletons) and rich,
varied prose (descriptions, dialogue, flavor). Pure procedural generation is reproducible but
bounded by hand-authored rules; pure LLM generation is rich but non-deterministic and hard to
constrain. How do we get both?

## Decision Drivers

- Reproducibility of world structure from a seed (debuggable, save/replayable).
- Rich, non-repetitive narrative content.
- The ability to regenerate prose without re-rolling structure, and vice versa.

## Considered Options

- Hybrid: deterministic procedural passes own structure; an LLM pass fills content.
- Pure procedural (grammars + templated text).
- Pure LLM (prompt the whole adventure).

## Decision Outcome

Chosen option: "hybrid". Procedural passes are a pure function of a seed and produce structure with
**empty content slots**; a separate, non-deterministic LLM pass transforms empty → filled and can
never invent structure. The two phases are physically separated in the pipeline; the intermediate
(structure-complete, content-empty) world is serializable and reproducible.

### Consequences

- Good, because structure is reproducible from `(seed)` and content is regenerable independently.
- Good, because the LLM cannot corrupt structure — enforced by types (it only sees content slots).
- Bad, because the model must carry a content-slot type on every content-bearing entity.

### Confirmation

The procedural phase runs entirely before the content phase; a golden-hash test asserts the same
seed yields byte-identical structure. LLM non-determinism is quarantined in content provenance.

## More Information

See ADR-0009 (embeddings/LLM providers) and the framework corpus (ADR-0004) that supplies the
structural vocabulary the procedural phase targets.
