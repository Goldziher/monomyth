# Architecture

The authoritative reference for how monomyth is decomposed. Open this first. See
[`vision.md`](./vision.md) for why the system is shaped this way, and
[`roadmap.md`](./roadmap.md) for when each piece lands.

## Planes, not a layer stack

monomyth is decomposed into **6 planes** plus **4 cross-cutting concerns**, not a linear layer stack.
A layer stack (sources → concepts → rules → config → contract → transforms → genre → rendering) would
force config and genre into the dependency path and pollute the contract. Planes model the real shape:
the contract is a hub, and config/genre are projections every transform reads, not layers between
other layers.

| Plane / concern | Crate(s) | Lifecycle | Notes |
|---|---|---|---|
| **P-KNOW** Knowledge (sources + RAG + concepts) | `monomyth-knowledge` | build+run | ship/reference/**user** collections; xberg substrate |
| **P-RULE** Rules / Frameworks / Laws | `monomyth-frameworks` + `artifacts/` | build-time | synthesized *vocabulary*; immutable at run-time |
| **P-CONTRACT** Narrative contract (the pivot) | `monomyth-core` | run-time, pure data | depends on nothing; no IO/async/LLM/config/genre |
| **P-TRANSFORM** Extraction + Generation | `monomyth-gen`, `monomyth-extract` (new) | run-time | inverse transforms over the contract |
| **P-RENDER** Rendering adapters | `monomyth-text`, future peers | run-time | thin, medium-specific, contract-only |
| **X-CONFIG** | `monomyth-config` (new) | cross-cutting | layered override *mechanism*; schemas co-located with consumers |
| **X-GENRE** | `monomyth-genre` (new) | cross-cutting | a config *dimension*, never in `core` |
| **X-LICENSE** | `monomyth-knowledge` ledger | cross-cutting | ship/reference/user three-layer enforcement |
| **X-DETERMINISM** | `monomyth-core` rng + `monomyth-gen` seeding | cross-cutting | seed → replay; BTree collections; snapshot-stable |

P-KNOW + P-RULE form the **build-time plane**. P-TRANSFORM + P-RENDER form the **run-time plane**.
Build-time output is synthesized (and reviewed) occasionally; run-time transforms execute per request.

## Crate dependency graph

```text
monomyth-core        pure DATA (World/Story/Content/NarrativeEdit) — public surface unchanged
monomyth-frameworks  pure RULE artifacts (include_str! JSON) — unchanged
monomyth-config      NEW: typed config SCHEMAS + layered override resolver (serde only)
monomyth-contracts   NEW: seam TRAITS only (Extractor, GenerationStrategy, GenreClassifier,
                     Renderer; re-exports VectorStore/Embedder/Llm). Depends on core + config types.
monomyth-knowledge   impls VectorStore/Embedder seam; ship/reference/user gate
monomyth-llm         impl Llm seam
monomyth-gen         impls GenerationStrategy (the pass pipelines); reads config
monomyth-synthesis   build-time: judge-gated reference→law drafting (composes knowledge + llm +
                     frameworks); output is human-reviewed, never a run-time dependency
monomyth-genre       NEW (Phase 2): GenreProfile + GenreClassifier impl
monomyth-extract     NEW (Phase 3): Extractor impl (text→World)
monomyth-render-*    NEW (Phase 5): Renderer impls, one per medium
monomyth-text/-cli   composition roots: select impls by config, inject trait objects
```

**Hard rule: `monomyth-core` depends on nothing.** No `-config`, `-genre`, or `-contracts` inbound
edge, ever. It is enforced by a `cargo tree` CI check. See
[`crates/monomyth-core/src/lib.rs`](../crates/monomyth-core/src/lib.rs) for the crate's own statement
of this invariant ("no IO, no async, no rendering, and no LLM/RAG concerns").

Across a plane boundary, code depends on `dyn Trait` / `impl Trait` from `monomyth-contracts`, never
on a concrete impl crate. Only composition roots (`monomyth-cli`, `monomyth-text`) name concrete impl
crates, and only to inject them. See
[`crates/monomyth-gen/src/lib.rs`](../crates/monomyth-gen/src/lib.rs) for the existing precedent —
`Generator` composes `dyn ProceduralPass` / `dyn ContentPass` pipelines, generalized by
`monomyth-contracts`'s `GenerationStrategy`.

## Build-time vs run-time

| | Build-time (P-KNOW + P-RULE) | Run-time (P-TRANSFORM + P-RENDER) |
|---|---|---|
| Produces | Rules, taxonomies, config defaults | A `World`, then a rendered medium |
| Frequency | Occasional; human-reviewed | Per request; automated |
| Grounded via | xberg RAG over source material | The contract + config + framework artifacts |
| Output | `artifacts/frameworks/*.json`, ledger entries | Serialized `World`, rendered text/media |
| Determinism | N/A (synthesis is not seeded) | Seed → replay; snapshot-stable |

Config (`X-CONFIG`) and genre (`X-GENRE`) are read by every run-time transform but are not
layers themselves — they cross-cut the run-time plane rather than sitting inside the build-time →
run-time pipeline.

## Invariants

Every row is enforced in code and/or CI, not just documented. Each links the ADR that decided it.

| Invariant | What it means | Enforcement | ADR |
|---|---|---|---|
| Determinism / BTree + seed-replay | `World` generation is a pure function of a `u64` seed; cross-referenced elements use `BTreeMap`/`BTreeSet`, never `Hash*`, so serialized output is byte-stable; a play session replays exactly from `(seed, action-log)` | Seed-42 snapshot tests; `RngState` sub-stream isolation in `monomyth-gen` | ADR-0002, ADR-0003, ADR-0013 |
| Ship/reference/user three-layer licensing | A source tagged `reference`/`noncommercial`/`copyright` may never live in the `ship` namespace or be surfaced verbatim; a future `user` namespace is architected but never auto-promoted to ship | Ledger declaration (`corpus/manifest.json`) + ingest-time refusal + retrieval-time `Filter::Eq("doc.metadata.namespace", "ship")` + CI ledger check | ADR-0005, ADR-0019 |
| Hybrid `Content::Empty`/`Content::Filled` seam | Every content-bearing field is a `Content` slot; procedural passes leave it `Empty`, a later content phase (LLM, or an extractor) fills it; structure and prose regenerate independently | `monomyth-core::content` type; procedural/content pass separation in `monomyth-gen` | ADR-0002, ADR-0014 |
| Config layered precedence + determinism-safety | `SystemDefault < DeploymentDefault < ProjectOverride < UserOverride`; a config value may change draws *within* a pass's RNG sub-stream but never the number or order of child-seed draws | `monomyth-config`'s `ConfigResolver`; a test that a changed value perturbs only its own sub-stream | ADR-0015 |
| Trait-first seams | Every plane boundary is a trait in `monomyth-contracts`; code depends on `dyn`/`impl Trait` across boundaries, never a concrete impl crate, except at composition roots | Code review; `cargo tree` boundary check | ADR-0013, ADR-0021 |
| Contract purity | `monomyth-core` is pure data: no IO, async, LLM, config, or genre types; any serialized-shape change bumps `SCHEMA_VERSION` under a stated back-compat policy | `#![forbid(unsafe_code)]`; `World::from_json_checked` schema-version gate; seed-42 snapshot tests | ADR-0014 |

## Where does X live

| If you're adding... | It lives in... | Not in... |
|---|---|---|
| A new vector-store / LLM / embedder backend | A feature-gated impl module implementing the trait (`monomyth-knowledge`, `monomyth-llm`) | `monomyth-contracts` (traits only) or `monomyth-core` |
| A tuning knob (a threshold, a top-k, a chance-permille) | `monomyth-config`, resolved through `Layered<T>` | A bare `const` scattered in `monomyth-gen` |
| A rule/taxonomy synthesized from source material | `artifacts/frameworks/*.json` (hand-encoded) or `artifacts/laws/*.json` (drafted by `monomyth-synthesis`, human-reviewed), read by `monomyth-frameworks` | `monomyth-config` (config tunes usage of rules; it does not encode them) |
| A genre concern (profile, classifier, targeting) | `monomyth-genre` | `monomyth-core` — never; a serialization-unchanged test + `cargo tree` assertion enforce this |
| A new plane-boundary seam (extractor, renderer, strategy, classifier) | A trait in `monomyth-contracts` | Ad hoc trait defined inside an impl crate |
| A new rendering medium | A new `monomyth-render-*` crate implementing `Renderer` | `monomyth-text` (stays one thin impl) |
| Extraction logic (text → `World`) | `monomyth-extract`, via `NarrativeEdit` + `validate()` | `monomyth-gen` (generation-only) or `monomyth-core` |
| Wiring which concrete impl runs | A composition root (`monomyth-cli`, `monomyth-text`) | Anywhere else — impl crates never name each other |
| A change to `World`/`Story`'s serialized shape | `monomyth-core`, its own reviewed commit, with a `SCHEMA_VERSION` bump | Riding along on a transform's feature branch |
| Source provenance / license namespace | The ledger, `crates/monomyth-knowledge/src/ledger.rs` | Anywhere ad hoc — the ledger is the single source of truth |

## Key file anchors

- [`crates/monomyth-core/src/lib.rs`](../crates/monomyth-core/src/lib.rs) — the pure pivot; states its
  own "no IO, no async" invariant; `SCHEMA_VERSION` lives in `world.rs`.
- [`crates/monomyth-gen/src/lib.rs`](../crates/monomyth-gen/src/lib.rs) — `Generator` and its
  seeded, sub-stream-isolated pass pipeline; the `GenerationStrategy` trait target.
- [`crates/monomyth-knowledge/src/ledger.rs`](../crates/monomyth-knowledge/src/ledger.rs) —
  `Namespace`/`Tier`, the license ledger backing X-LICENSE.
- `artifacts/frameworks/*.json` — the P-RULE plane's output; where synthesized `system`-tier law
  artifacts land (see [`methodology.md`](./methodology.md)).
- `adrs/0013`–`0021` — the architecture decisions this document mirrors. Read the ADRs for the
  reasoning; read this document for the current map.

## Related ADRs

| ADR | Decision |
|---|---|
| [0013](../adrs/0013-trait-first-planes-architecture.md) | Trait-first, planes-over-layers architecture |
| [0014](../adrs/0014-contract-as-extraction-generation-pivot.md) | The contract as extraction⇄generation pivot |
| [0015](../adrs/0015-layered-override-configuration.md) | Layered override config (`monomyth-config`) |
| [0016](../adrs/0016-build-time-methodology-and-law-synthesis.md) | `map→synthesize→configure` methodology + reference-ingest law synthesis |
| [0017](../adrs/0017-genre-as-config-dimension.md) | Genre as a cross-cutting config dimension |
| [0018](../adrs/0018-extraction-subsystem.md) | Extraction subsystem (`monomyth-extract`) |
| [0019](../adrs/0019-user-uploaded-media-namespace.md) | User-uploaded media + the `user` trust domain |
| [0020](../adrs/0020-medium-agnostic-rendering-adapters.md) | Medium-agnostic rendering adapters (`Renderer` trait) |
| [0021](../adrs/0021-rust-architecture-guidelines-in-ai-rulez.md) | Rust architecture guidelines in `.ai-rulez` |
| [0024](../adrs/0024-llm-judge-feedback-loop-synthesis.md) | LLM-as-judge feedback loop for reference→law synthesis |
| [0025](../adrs/0025-reference-retrieval-quality.md) | Reference retrieval quality: dedup, multi-query coverage (hybrid deferred) |
