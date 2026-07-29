---
title: The narrative graph
description: NarrativeStructure as a single-source, acyclic, reconverging DAG, its edit vocabulary, and how a player traverses it.
---

The narrative is a first-class, serializable **`NarrativeStructure`** — a single-source, acyclic,
**reconverging DAG** of beats, not a flat list.

## Why a graph, not a list

Player choices fork the trunk and reconverge, so the spine is a real branching structure: fork
diamonds open at the corpus-flagged *optional* Campbell stages, not at arbitrary points a designer
picked by hand. `NarrativeNode`s hold `NarrativeEdge`s directly, so forward root→ending traversal is
the cheap path; reconvergence is simply two nodes pointing at the same target.

Entities are stored once in per-kind `SlotMap`s; everything else references them by typed, `Copy`,
generational keys (`LocationId`, `EntityId`, `NarrativeNodeId`, …) rather than by index or by
`Rc<RefCell<_>>`. IDs are small, serializable, and generational, so cycles are trivial (edges are just
IDs), the whole `World` round-trips cleanly through JSON, and stale references are caught rather than
silently dereferenced.

Each node's authorial role — `NodeKind`: origin, beat, branch, merge, or ending — is *derived* from
topology, not hand-set. A node is a branch because it has multiple outgoing edges, not because
something labeled it one.

## The edit vocabulary

The structure is mutated only through a serializable **edit vocabulary**, `NarrativeEdit`. Applying a
batch (`apply_edits`) is transactional: clone, apply, recompute node kinds, validate, and commit — or
roll back the whole batch on any failure. This is deliberate: generation, human edits
(`monomyth edit`), and future LLM agents all drive the *same* validated surface, so no path can leave
the graph in a state the others wouldn't also produce.

`NarrativeStructure::validate` enforces the DAG invariants: single source, acyclic, every node
reachable from the source, every node reaches an ending, and each node's `NodeKind` matches its
topology. `World::validate` / `World::from_json_checked` enforce whole-world referential integrity at
the load boundary on top of that.

## Traversal

Traversal is a pure engine action, `Action::Choose`, advancing a cursor along available, optionally
guarded edges. It is the same `apply(world, action) -> Vec<Event>` engine every other action goes
through — walking the story is not a special case bolted onto the model.

## Where this is used

- `monomyth-gen`'s procedural passes grow the spine and splice beats onto it through
  `NarrativeEdit`, drawing from a seeded RNG sub-stream — see
  [Hybrid generation](/monomyth/concepts/hybrid-generation/).
- `monomyth edit --world world.json --script edits.json` applies a hand-authored or tool-generated
  JSON script of `NarrativeEdit` operations, re-validated transactionally.
- `monomyth-extract`'s classify-only slice re-derives node stage classifications through
  `NarrativeEdit::SetNodeStage`, reusing the same write surface generation uses — never a parallel
  one. See [ADR-0018](https://github.com/Goldziher/monomyth/blob/main/adrs/0018-extraction-subsystem.md).

See [ADR-0003](https://github.com/Goldziher/monomyth/blob/main/adrs/0003-slotmap-graph-domain-model.md)
for the full decision record.
