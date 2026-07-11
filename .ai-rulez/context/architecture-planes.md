# Architecture: planes and cross-cutting concerns

monomyth is a configurable, layered, **bidirectional** engine for narrative structure: it moves
between unstructured narrative and a structured, medium-agnostic representation in both directions —
**extraction** (text→contract) and **generation** (contract→text/media) — for any medium, grounded
in source material via xberg RAG. The serializable narrative contract (`monomyth-core`'s
`World`/`Story`) is the pivot; extraction and generation are inverse transforms over it.

The working method is `map/extract concept → synthesize into rule(s) → translate into configuration`:
a **build-time** plane produces rules/config from grounded concepts; the **run-time** plane consumes
them. The mythology frameworks are the first instance of this method, not the whole product.

## Planes (each boundary is a trait; ADR-0013)

| Plane | Crate(s) | Lifecycle |
|---|---|---|
| P-KNOW — Knowledge (sources + RAG + concepts) | `monomyth-knowledge` | build + run |
| P-RULE — Rules / Frameworks / Laws (synthesized vocabulary) | `monomyth-frameworks` + `artifacts/` | build-time |
| P-CONTRACT — Narrative contract (the pivot, pure data) | `monomyth-core` | run-time |
| P-TRANSFORM — Extraction + Generation | `monomyth-gen`, `monomyth-extract` | run-time |
| P-RENDER — Rendering adapters (thin, per medium) | `monomyth-text` + peers | run-time |

## Cross-cutting concerns (read by every plane; not layers)

- **X-CONFIG** (`monomyth-config`) — layered override resolution (`SystemDefault < DeploymentDefault
  < ProjectOverride < UserOverride`); schemas co-located with consumers; determinism-safe values only.
- **X-GENRE** (`monomyth-genre`) — a config dimension (`GenreProfile` + `GenreClassifier`); never a
  type in `monomyth-core`.
- **X-LICENSE** (the ledger) — `ship` / `reference` / `user` namespaces; three-layer enforcement.
- **X-DETERMINISM** (core RNG + gen seeding) — seed→replay; BTree collections; snapshot-stable.

## Seam traits live in `monomyth-contracts`

`monomyth-core` stays pure data (no IO/async), so async/IO seam traits (`Extractor`,
`GenerationStrategy`, `GenreClassifier`, `Renderer`, plus the existing `VectorStore`/`Embedder`/`Llm`)
live in a dedicated `monomyth-contracts` crate. Across a plane boundary depend on `dyn Trait` /
`impl Trait`, never a concrete impl crate; only composition roots (`monomyth-cli`, `monomyth-text`)
name concrete impls. **`monomyth-core` depends on nothing.**
