---
title: Hybrid generation
description: Deterministic procedural structure and a quarantined LLM content pass — reproducible from a seed, content fills slots but never invents structure.
---

Adventures need both reproducible structure (maps, stats, pacing, quest skeletons) and rich, varied
prose (descriptions, dialogue, flavor). Pure procedural generation is reproducible but bounded by
hand-authored rules; pure LLM generation is rich but non-deterministic and hard to constrain.
monomyth uses both, kept physically separate.

## The two phases

**Procedural passes** are a pure function of a seed. They produce structure with **empty content
slots** — the branching narrative spine and its beats, then the map, cast, and items — each drawing
from its own `ChaCha8` sub-stream. The narrative passes build the story graph through the same
serializable edit vocabulary that human input and future agents use (see
[The narrative graph](/monomyth/concepts/narrative-graph/)), so structure growth is inspectable and
validated the same way regardless of who or what is growing it.

**The content pass** is non-deterministic and takes no RNG. It transforms empty slots into filled
ones by retrieving ship-safe corpus passages and prompting an LLM through `monomyth-llm`. It touches
no structure — it can rename a description, never add a room or fork a choice.

```text
seed ──▶ procedural passes ──▶ World (structure complete, content empty)
                                        │
                                        ▼
                          content pass (LLM, grounded, no RNG)
                                        │
                                        ▼
                             World (structure complete, content filled)
```

## Why this split

- **Reproducibility.** The same seed always reproduces a byte-identical world structure. A play
  session replays exactly from `(seed, action-log)`.
- **Independent regeneration.** Content can be regenerated — a different prompt, a different model —
  without re-rolling structure, and vice versa.
- **A hard safety boundary.** The LLM cannot corrupt structure: it is enforced by types (the content
  pass only ever sees `Content` slots, never the graph) and by the content passes carrying no
  procedural RNG.

Every content-bearing entity carries a `Content` slot — `Content::Empty` or `Content::Filled` —
alongside its structural fields. Procedural passes leave the slot `Empty`; the content phase (or,
later, an extractor) fills it. This is the same seam extraction reuses to leave content it cannot
derive from source text as `Empty`, rather than inventing a parallel mechanism.

## Determinism guarantees

- Cross-referenced elements use `BTreeMap`/`BTreeSet`, never `Hash*`, so serialized output is stable
  and snapshot-testable.
- A golden-hash test asserts the same seed yields byte-identical structure; a fingerprint test asserts
  the content phase leaves structure byte-unchanged.
- A config value may change the draws taken *within* a pass's RNG sub-stream, but never the number or
  order of child-seed draws — changing that would silently break replay for every downstream pass.

## In the CLI

```sh
# Structure only, deterministic:
cargo run -p monomyth-cli -- gen --seed 42

# Structure, then the LLM content pass fills prose (requires provider config):
cargo run -p monomyth-cli -- gen --seed 42 --fill
```

See [ADR-0002](https://github.com/Goldziher/monomyth/blob/main/adrs/0002-hybrid-generation.md) and
[ADR-0003](https://github.com/Goldziher/monomyth/blob/main/adrs/0003-slotmap-graph-domain-model.md).
