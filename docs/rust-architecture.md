# Rust architecture guidelines (mirror)

> **Source of truth: `.ai-rulez/rules/`** — this document mirrors those rules for human readers. Edit
> the ai-rulez source, not this file or the generated `CLAUDE.md`/`AGENTS.md`. Regenerate the assistant
> files with `ai-rulez generate` after editing a rule.

Companion to [ADR-0021](../adrs/0021-rust-architecture-guidelines-in-ai-rulez.md). See
[`architecture.md`](./architecture.md) for the plane map these rules enforce.

## 1. Trait-first seams

Source: `.ai-rulez/rules/trait-first-architecture.md`.

- Every **plane boundary** is a trait, generalizing the existing `Arc<dyn VectorStore>` /
  `dyn ProceduralPass` / `dyn ContentPass` / `dyn Embedder` seams. Seam traits live in
  `monomyth-contracts`, not `monomyth-core` — the contract stays pure data with no IO/async.
- **Depend on abstractions across boundaries.** Use `dyn Trait` / `impl Trait` from
  `monomyth-contracts`, never a concrete impl crate. Impl crates must not depend on each other across
  plane boundaries. Only composition roots (`monomyth-cli`, `monomyth-text`) name concrete impls, and
  only to inject them. Impls are selected by configuration.
- **The contract depends on nothing.** `monomyth-core` is a pure serializable pivot: no IO, async,
  LLM, config, or genre. A `cargo tree` check asserts `monomyth-core` has no inbound
  `monomyth-config`/`monomyth-genre`/`monomyth-contracts` dependency.
- **Config is not a rule artifact.** Framework JSON (`artifacts/frameworks/*.json`) is build-time
  ground truth the code conforms to; `monomyth-config` is run-time tunables. Config may select or
  weight an artifact, never edit it.
- Trait-ify **only plane boundaries**, never within a crate.

## 2. Module & visibility discipline

Source: `.ai-rulez/rules/api-visibility.md`, `.ai-rulez/rules/module-size-cap.md`.

- **`pub(crate)` by default.** Expose the smallest surface callers need. A type or function becomes
  `pub` only when it is genuinely part of the crate's API contract.
- **Document every public item.** Every genuinely-`pub` item (type, function, constant, trait) carries
  a rustdoc comment explaining *why* it exists. Public examples use `?`, not `.unwrap()`. Document
  errors, panics, and safety invariants.
- **Module-size cap.** When a file approaches ~1000 lines, split it by concern (a domain aggregate, its
  operations, its queries are natural boundaries) rather than let it grow.
- **Newtypes at boundaries.** Prefer newtypes over bare primitives for public parameters and IDs
  (typed slotmap keys, not raw indices); avoid `bool` parameters — use enums.
- **Keep struct fields private** unless the type is a plain serializable data record (the domain
  contract's `serde` types are the deliberate exception).

## 3. Performance patterns, determinism-first

Source: `.ai-rulez/rules/performance-determinism.md`.

The deterministic-serialization contract dominates every performance choice: serialized output must be
byte-stable and snapshot-testable. Optimize within that constraint, never against it.

- **Never** swap `BTreeMap`/`BTreeSet` → `HashMap`/`HashSet` in or feeding serialized output —
  iteration order *is* the deterministic contract.
- `ahash`/`foldhash` are **non-patterns** here (transitive deps only) — use a faster hasher only for
  an internal, non-serialized, order-irrelevant hot map, with a `//` comment justifying it.
- **`memchr`** for byte/substring scanning over large corpus text. The concrete win today is
  single-pass CRLF→LF normalization in `acquire/normalize.rs` (~15–25% on multi-MB books).
- **Compile regex once** via `LazyLock` — never per call.
- **Bounded streaming** for network bodies (the acquire HTTP path already caps at 256 MB); preallocate
  when the size is known.
- **SIMD/vector math is xberg/ONNX's domain** — don't hand-roll it.
- Config values must be **determinism-safe**: they may change draws *within* a pass's RNG sub-stream
  but must never change the number or order of child-seed draws (ADR-0015).

## Full guideline set

These three groups sit alongside the pre-existing Rust conventions in `CLAUDE.md`/`AGENTS.md`
(edition 2024, `thiserror`/`anyhow`, no `unwrap()` in library code, `tracing` over `println!`, and so
on) — this document covers only what changed with the planes architecture. For the complete set of
generated rules, read `CLAUDE.md` or `AGENTS.md` at the repository root; both are generated from
`.ai-rulez/` and must never be hand-edited.
