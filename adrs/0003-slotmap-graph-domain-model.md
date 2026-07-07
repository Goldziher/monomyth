---
status: accepted
date: 2026-07-07
decision-makers: Na'aman Hirschfeld
---

# Slotmap graph domain model (not ECS)

## Context and Problem Statement

The world model is a graph — locations link to locations, items sit in containers, quests
reference entities, and the **story spine itself is a branching, reconverging graph** of beats
(player choices fork it and later rejoin). It must serialize cleanly (it is the shared contract,
ADR-0001), round-trip through JSON, and be diffable and snapshot-testable. What in-memory
representation do we use?

## Decision Drivers

- Clean serialization (round-trip, stable IDs across save/load).
- Cheap, safe cross-references (cycles, back-pointers) without aliasing pain.
- Turn-based, modest entity counts with rich relationships — not a real-time frame loop.

## Considered Options

- Slotmap-based graph with typed, generational keys.
- ECS (`hecs` / `bevy_ecs`).
- Plain structs with `Rc<RefCell<…>>` references.

## Decision Outcome

Chosen option: "slotmap graph". Entities are stored once in per-kind `SlotMap`s; everything else
references them by typed, `Copy`, generational keys (`LocationId`, `EntityId`, `NarrativeNodeId`, …).
Deterministic `BTreeMap`/`BTreeSet` for stable serialized ordering.

The story spine is a `NarrativeStructure`: a single-source, acyclic, **reconverging DAG**
(`NarrativeNode` + `NarrativeEdge`, edges held on the node so forward root→ending traversal is the
cheap path; reconvergence is just two nodes pointing at the same target). Each node's authorial role
(`NodeKind` — origin / beat / branch / merge / ending) is *derived* from topology, not hand-set. The
structure is mutated only through a serializable **edit vocabulary** (`NarrativeEdit`): `apply_edits`
is transactional — clone, apply, recompute kinds, validate, and commit *or roll back* — so
generation, human edits, and future LLM agents all drive one validated surface.

### Consequences

- Good, because IDs are small, serializable, and generational (stale refs are caught).
- Good, because cycles are trivial (edges are just IDs) and the whole `World` round-trips cleanly.
- Good, because one serializable edit vocabulary lets generation, human input, and agents transform
  the narrative graph through the same validated, replayable operations.
- Bad, because we forgo ECS's cache-friendly bulk iteration — irrelevant for a turn-based engine.

### Confirmation

`serde` round-trip and determinism tests: a hand-authored `World` re-serializes byte-identical, and
`(seed, action-log)` replays identically. `NarrativeStructure::validate` enforces the DAG invariants
(single source, acyclic, all-reachable, every node reaches an ending, kind matches topology);
`World::validate` / `World::from_json_checked` enforce whole-world referential integrity at the load
boundary; a failed edit batch leaves the structure byte-identical (transactional roll-back).

## Pros and Cons of the Options

### ECS

- Good, because excellent for many entities iterated per frame.
- Bad, because serialization is awkward (component registration, entity-id remapping) and it pushes
  system-scheduling machinery we don't need.

### `Rc<RefCell<…>>`

- Bad, because it doesn't serialize cleanly and creates aliasing/borrow and cycle headaches.
