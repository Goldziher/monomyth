---
priority: high
---

# Trait-first, planes-over-layers architecture

monomyth is organized as **6 planes** — Knowledge (`monomyth-knowledge`), Rules
(`monomyth-frameworks` + `artifacts/`), Contract (`monomyth-core`), Transform (`monomyth-gen`,
`monomyth-extract`), Render (`monomyth-text` + peers) — plus **4 cross-cutting concerns**: Config
(`monomyth-config`), Genre (`monomyth-genre`), License (the ledger), Determinism (core RNG + gen
seeding). See ADR-0013 and `docs/architecture.md`.

- **Trait-first seams.** Every plane boundary is a trait, generalizing the existing
  `Arc<dyn VectorStore>` / `dyn ProceduralPass` / `dyn ContentPass` / `dyn Embedder` seams. Seam
  traits live in `monomyth-contracts` (not `monomyth-core` — the contract stays pure data with no
  IO/async). Trait-ify **only plane boundaries**, never within a crate.
- **Depend on abstractions across boundaries.** Use `dyn Trait` / `impl Trait` from
  `monomyth-contracts`, never a concrete impl crate. Impl crates must not depend on each other across
  plane boundaries. Only composition roots (`monomyth-cli`, `monomyth-text`) name concrete impls, and
  only to inject them. Impls are selected by configuration.
- **The contract depends on nothing.** `monomyth-core` is a pure serializable pivot: no IO, async,
  LLM, config, or genre. Config and genre reach transforms as values and trait objects, never as new
  fields on `World`/`Story`. A `cargo tree` check asserts `monomyth-core` has no inbound
  `monomyth-config`/`monomyth-genre`/`monomyth-contracts` dependency.
- **Config ≠ rule artifacts.** Framework JSON (`artifacts/frameworks/*.json`) is build-time ground
  truth the code conforms to; `monomyth-config` is run-time tunables. Config may select or weight an
  artifact, never edit it.
