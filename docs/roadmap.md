# Roadmap

A living document. Phases are sequential; each states its goal, key tasks, exit criteria, and the
determinism/licensing constraints it must not violate. See [`architecture.md`](./architecture.md) for
the target shape and [`methodology.md`](./methodology.md) for the build-time procedure Phase 1 lands.

## Ordering justification

The **config spine leads** because everything downstream reads config — genre (Phase 2), extraction
(Phase 3), and rendering (Phase 5) all depend on it existing first. It is fused with the near-term
focus (deepening generation grounding) because grounding depth and config are the same surface:
`GROUNDING_TOP_K` *is* a config value. Genre-targeting comes next because it is cheap and
contract-safe, validating the config dimension before the riskier extraction work. Extraction follows
the contract-stability ADR (0014) and config spine that constrain it. User uploads implement an
already-decided seam (ADR-0019). Rendering generalization is last — least blocking, and benefits from a
stable contract and config.

## Phase 0 — Documentation-first (no code)

**Goal:** write the plane model, the frozen-contract rule, and the config boundary down before any new
crate exists, so scope creep and contract churn have something to be checked against.

**Key tasks:**

- ADRs 0013–0021 (MADR format), indexed in `adrs/README.md`.
- `docs/{vision,architecture,methodology,roadmap,rust-architecture}.md` (this directory).
- `README.md` reframed from the text-adventure framing to the engine framing.
- `.ai-rulez/rules/` + `.ai-rulez/context/` authored (three guideline groups + plane map);
  `ai-rulez generate` regenerates `CLAUDE.md`/`AGENTS.md` (run by the user, not by an agent).

**Exit criteria:** all nine ADRs present and indexed; all five docs exist; `.ai-rulez` rules authored
and generation confirmed by the user; review for coherence with the plane model.

**Constraints:** no code changes. Nothing in this phase touches `monomyth-core`, so determinism and
licensing are unaffected by definition.

## Phase 1 — Config spine + generation grounding + finish F/G/H + land I

**Goal:** stand up the config mechanism through one real value end-to-end, then widen it, while
deepening generation grounding and landing the queued acquisition work. Fuses the "config spine first"
ordering with the "generation grounding first" near-term-focus decision.

**Key tasks:**

- **1a. Config vertical slice (de-risk first).** Create `monomyth-config` (`Layered<T>`,
  `LayerSource`, `ConfigResolver`; deps `serde`/`schemars`) and a `monomyth-contracts` skeleton.
  Migrate exactly one value end-to-end: `NarrativeConfig::fork_chance_permille`
  (`gen/src/passes/backbone.rs`) from a constant to a resolved layered value, wired through the CLI.
- **1b. Generalize config** to the other knobs: `BeatConfig` (`beat.rs`), `MIN_ROOMS`/`MAX_ROOMS`
  (`map.rs`), quest bounds (`backbone.rs`), `GROUNDING_TOP_K` (`content.rs`) — each behind the
  resolver with a "default equals old constant" snapshot guard.
- **1c. Express pass-pipeline selection as a config-driven `GenerationStrategy` trait**, generalizing
  `Generator::with_pipelines`; keep `dyn ProceduralPass`/`dyn ContentPass` as-is (already trait-first).
- **1d. Deepen generation grounding.** Make `ground`/`build_prompt` (`gen/src/content.rs`)
  config-tunable (top-k, per-`ContentKind` templates, multi-hint retrieval); stays ship-gated via
  `KnowledgeQuery::surfaceable`. Optionally wire the Doty dimension-checklist law (from task I) into
  content-fill prompts.
- **1e. Finish queued acquire work.** F = OpenDAL storage refactor (ADR-0012) + `corpus/raw/{ship,
  reference}` split; G = re-namespace `gutenberg_english`/`pg19` → reference + full-download inspect
  mode; H = acquire hardening (4xx-no-retry, response-size cap, `FilteredOut` classification — verify
  and land). Apply the `memchr` CRLF win in `normalize.rs`.
- **1f. Land I — reference-ingest law synthesis.** See [`methodology.md`](./methodology.md#worked-example-2-reference-ingest-law-synthesis)
  for the full worked example: a reference-collection ingest path, curated myth-theory sources, and
  synthesized `Tier::System` law artifacts (Laurasian/Gondwanan two-arc, binary-opposition mediation,
  trifunctional faction casting, Doty dimensions) committed beside `artifacts/frameworks/`.

**Exit criteria:** build a small ship corpus, generate a world with layered config overrides, confirm
grounded content-fill; reference-ingest a myth-theory source and confirm it stays reference-only.

**Constraints:**

- **Determinism:** seed-42 snapshot byte-identical when a resolved config value equals the old
  default; a changed value perturbs only its own pass's RNG sub-stream, never downstream passes.
  Nothing in Phase 1 touches `monomyth-core`, so determinism stays frozen.
- **Licensing:** reference ingest writes only to `REFERENCE_COLLECTION`; synthesized artifacts are
  `Tier::System` (no prose); every synthesis is human-reviewed pre-commit; ledger invariant tests cover
  the new sources; the existing three-layer enforcement stays intact, not bypassed.

## Phase 2 — Genre as a config dimension (targeting only)

**Goal:** prove config can steer output toward a non-myth genre without touching the contract.

**Key tasks:** new `monomyth-genre` crate: `GenreProfile` config + `GenreClassifier` trait (input role,
stub impl). Wire a genre *targeting* parameter into generation prompt-building and `monomyth-text`.
Classification itself is deferred.

**Exit criteria:** a non-myth profile (e.g. detective) shifts output measurably; the contract is
unchanged.

**Constraints:** no genre type in `monomyth-core` — enforced by a serialization-unchanged test and a
`cargo tree` assertion (core has no `-genre` inbound edge).

## Phase 3 — Extraction vertical slice

**Goal:** prove the contract can hold extracted structure, on the smallest possible slice, before
scaling extraction.

**Key tasks:** new `monomyth-extract` crate: an `Extractor` producing a *minimal valid* `World`
(single node/location) from a short text, using `NarrativeEdit` as the write surface, `validate()` as
the gate, framework vocabulary, and the Phase 2 genre prior. Genre lands before extraction so
extraction consumes a stable schema/prior selector.

**Exit criteria:** the pivot is proven bidirectionally on one worked example before scaling further.

**Constraints (contract gate, key de-risk):** if the slice shows the contract can't hold extracted
structure, route the fix through ADR-0014's `SCHEMA_VERSION` gate — never a silent core hack.

## Phase 4 — User uploads implementation

**Goal:** implement the `user` trust-domain seam that ADR-0019 architects but defers.

**Key tasks:** add `Namespace::User` + `Tier::UserLicensed` (`ledger.rs`, mirroring `as_wire`,
updating the wire/serde and `dangerous_tiers_are_reference_only` tests to include `user`); wire
upload → ledger entry → `user` collection; retrieval treats `user` as non-surfaceable-by-default with
explicit per-project promotion.

**Exit criteria:** a user-uploaded source is ingested, ledgered, and retrievable only under explicit
promotion.

**Constraints:** the `user` namespace is never auto-promoted to `ship`. All existing licensing
enforcement extends to cover three trust domains, not two.

## Phase 5 — Rendering generalization

**Goal:** prove the `Renderer` trait seam with a second medium.

**Key tasks:** introduce the `Renderer` trait (ADR-0020); refactor `monomyth-text` into one thin
`Renderer` impl behind it; add a second medium (e.g. structured game/LitRPG) driven by genre targeting.

**Exit criteria:** two renderers over the same `World`, selected by config.

**Constraints:** renderers stay thin and contract-only — no medium-specific concerns leak back into
`monomyth-core`.

## Out of scope / not yet

Genre classification (Phase 2 is targeting only); full extraction beyond the one-node slice (Phase 3);
user-upload UI/fetchers; a pixel/graphical frontend; multiplayer/persistent worlds; the pgvector
adapter implementation (ADR-0011, deferred).

## Verification (every commit)

- Local check set: `cargo fmt` · `cargo clippy --workspace --all-targets --tests -D warnings` ·
  `cargo test --workspace` · `poly fmt --fix . && poly lint .`; the artifact validator when framework
  artifacts change.
- Determinism: seed-42 snapshot byte-identical before/after each config migration; a changed config
  perturbs only its intended sub-stream; the content phase leaves structure byte-unchanged.
- Boundaries: `cargo tree` asserts `monomyth-core` has no `-config`/`-genre`/`-contracts` inbound
  pollution; default-feature `monomyth-gen` pulls no HTTP stack.
- Licensing: `monomyth corpus audit` green; reference sources never surface
  (`is_surfaceable`); synthesized law artifacts are `Tier::System`.
