---
status: accepted
date: 2026-07-11
decision-makers: Na'aman Hirschfeld
---

# Layered override configuration system (monomyth-config)

## Context and Problem Statement

Tuning values are scattered as constants across `monomyth-gen` (`fork_chance_permille`, `MIN_ROOMS`/
`MAX_ROOMS`, `GROUNDING_TOP_K`, …). The product needs both a deployment audience (operators set safe
defaults) and a per-project/per-user audience (override those defaults) — "config" cannot mean a
single flat file. How do we resolve typed configuration from ordered layers without turning
"configuration" into a catch-all that also swallows rule artifacts and strategy selection?

## Decision Drivers

- Two config audiences, layered: deployment defaults and project/user overrides must compose with a
  clear precedence, not a single global file each caller edits directly.
- Config schemas are owned by the pass that consumes them (`NarrativeConfig` already lives in
  `monomyth-gen`); a config crate must not become a second definition of every tunable type.
- Determinism (ADR-0002, ADR-0003): a config value must never silently change *which* or *how many*
  RNG sub-stream draws a pass takes — only the draws *within* a sub-stream it already owns.
- Three things already get called "config" informally and must not collapse into one: rule artifacts
  (`artifacts/frameworks/*.json`), tuning parameters, and strategy/impl selection.
- This is the single biggest scope-creep risk in the roadmap (a config DSL or computed config would
  swallow unrelated design space) and needs a bounded ADR before any code exists.

## Considered Options

- A new `monomyth-config` crate owning only the resolution *mechanism* (`Layered<T>`,
  `ConfigResolver`, `LayerSource`), with schemas co-located in their consuming crates.
- A single flat config file/struct covering every tunable, owned centrally.
- A general-purpose config/expression language (computed values, conditionals) for maximum flexibility.

## Decision Outcome

Chosen option: "a new `monomyth-config` crate, mechanism only". It resolves typed configuration from
ordered layers with precedence

```text
SystemDefault < DeploymentDefault < ProjectOverride < UserOverride
```

(later layers win). The crate exposes `Layered<T>` (a resolved value plus which layer it came from,
for provenance/debugging), a `ConfigResolver` that merges layers, and the `LayerSource` enum — nothing
else. **Config schemas stay co-located with their consuming pass** — `NarrativeConfig` stays in
`monomyth-gen`, a future `GenreProfile` stays in `monomyth-genre` — `monomyth-config` never defines a
domain-specific config type itself; it is generic (`serde` + `schemars`) over whatever type a
consumer hands it.

**Determinism-safety is a hard constraint, not a convention:** a config value may change the draws
*taken within* a pass's existing RNG sub-stream, but must never change the number or order of
child-seed draws — changing that silently breaks replay for every downstream pass. Config also **never
edits framework rule artifacts**: it may select or weight an artifact entry (e.g. which Campbell stage
set to target) but may never override the artifact's own values.

**Three kinds of "config," kept distinct:**

1. **Rule artifacts** (`artifacts/frameworks/*.json`) — build-time ground truth the code conforms to.
   Not resolved by `monomyth-config`; owned by ADR-0004/ADR-0016.
2. **Tuning parameters** (`fork_chance_permille`, `MIN_ROOMS`/`MAX_ROOMS`, `GROUNDING_TOP_K`) — what
   `monomyth-config` actually layers.
3. **Strategy/impl selection** (which `GenerationStrategy`/backend runs) — a config *value* selects
   the impl, but the selection mechanism itself is the ADR-0013 trait seam, not this crate.

### Consequences

- Good, because deployment operators and individual projects/users get independent, composable
  override points without editing each other's layer.
- Good, because config schemas staying with their consumers means `monomyth-gen` still owns
  `NarrativeConfig`'s meaning; `monomyth-config` cannot drift out of sync with what it configures.
- Good, because the determinism constraint is stated once, here, instead of re-derived per config
  migration.
- Bad, because every existing constant migrated to a layered value needs a "default equals the old
  constant" snapshot test to prove the migration is a no-op; mitigated by migrating one value first
  (ADR-0013's Phase 1a vertical slice) before generalizing.

### Confirmation

A test resolves a changed config value and asserts downstream RNG sub-streams outside the owning pass
are byte-identical (only the intended sub-stream's draws differ). A second test resolves the default
layered config and asserts the seed-42 snapshot matches the pre-config-migration output exactly. Code
review enforces that no PR adds a domain-specific config type to `monomyth-config` itself.

## Pros and Cons of the Options

### `monomyth-config` as mechanism only, schemas co-located

- Good, because it has no opinion on what gets configured — it stays a small, stable dependency for
  every future config-consuming crate (`gen`, `genre`, later `extract`/`render`).
- Good, because the precedence chain is the one piece of logic every consumer would otherwise
  reimplement inconsistently.
- Neutral, because callers must still explicitly wire their schema through the resolver — no free
  auto-discovery, by design.

### Single flat config file/struct

- Bad, because a central struct covering every tunable re-creates the coupling ADR-0013's planes
  exist to avoid — `monomyth-gen` and a future `monomyth-genre` would both need to touch one shared
  type for unrelated changes.

### General-purpose config/expression language

- Bad, because computed/conditional config is exactly the determinism hazard this ADR is meant to
  foreclose — a config expression could alter control flow (and therefore RNG draw count) in ways no
  snapshot test would catch until it already shipped.
- Bad, because it is unbounded scope for no near-term requirement.

## More Information

Draws the boundary against ADR-0004 (frameworks-as-schema): rule artifacts are explicitly out of
`monomyth-config`'s scope. Consumed by ADR-0017 (genre as a config dimension). Instantiates the
X-CONFIG cross-cutting concern of ADR-0013. The vertical-slice migration order (one value, then
generalize) is tracked as roadmap Phase 1a/1b, not part of this decision.

## Implementation note (2026-07-12): the mechanism + first value landed

The `monomyth-config` crate now exists with the mechanism-only surface this ADR specifies —
`Layered<T>`, `LayerSource`, `ConfigResolver` — and the Phase 1a vertical slice
(`generation.fork_chance_permille`) is threaded end to end. The immutable decision above stands; two
points of record:

- **A fifth layer, `RuntimeOverride`, was added above `UserOverride`** for CLI flags / programmatic
  overrides, so the precedence is
  `SystemDefault < DeploymentDefault < ProjectOverride < UserOverride < RuntimeOverride`. A `--flag`
  therefore always wins over any file, which is what the existing global `--model`/`--db` flags need
  once they migrate to config-supplied defaults.
- **The determinism constraint is enforced exactly as the Confirmation section requires.**
  `monomyth-gen` keeps owning `NarrativeConfig` and gains only a flat, resolved `GenerationConfig`
  (plain scalars) plus `Generator::with_config`; it takes *no* dependency on `monomyth-config` (the CLI
  does the projection). `with_default_passes()` now delegates to
  `with_config(&GenerationConfig::default())`, and three ratchets pin the no-op: two in
  `monomyth-gen/tests/determinism.rs` (the config-default and an explicit `fork_chance = 500` both
  reproduce the seed-42 golden `0x65f6_6a4b_a541_5419`) and one cross-crate test in `monomyth-cli`
  binding `monomyth-config`'s system default to that golden.

The file schema deserializes via `serde` + `toml` (the `deny_unknown_fields` guard makes a misspelled
key a hard error). Sections landed so far: `[generation]` (fork probability + the beat/map/item/cast
procedural bounds), `[models]` (per-task `content`/`synthesis` routing, replacing the CLI's hardcoded
model consts — model ids now live only in `ModelsSettings::default()`), and `[synthesis]` (the judge
loop's integer knobs, bound to `LoopConfig::default()` by a `monomyth-cli` test). `monomyth.example.toml`
documents the whole schema. Still deferred: `[retrieval]`/`[paths]`, the synthesis f64 bar thresholds,
the extraction softmax temperature, a dedicated judge model (needs `draft_law`'s API widened), and the
`schemars` JSON-Schema emission.

## Remediation note (2026-07-12): review hardening

A post-ship critical review found gaps that were closed before building the next workstream on this
spine:

- **`resolve()` is now fallible and validates.** `ConfigResolver::resolve(self) -> Result<MonomythConfig,
  ConfigError>` runs `MonomythConfig::validate()` after every layer (including runtime overrides), so an
  out-of-range value (a permille over `1000`, an inverted `min`/`max`) is rejected at the resolver
  boundary — the Confirmation section's determinism-safety plus the input-validation rule — rather than
  reaching a pass where it could panic or empty an RNG draw. The new `ConfigError::Invalid { field,
  reason }` names the offending `section.key`, and both `Read`/`Parse` `Display` strings now interpolate
  their `{source}` so a malformed `monomyth.toml` reports the underlying cause, not just the filename.
- **`play` honors config.** The behavioral bug that mattered most: `gen --seed N` generated through
  resolved config while `play --seed N` regenerated through hardcoded defaults, so a non-default
  `[generation]` table made the two diverge and broke `(seed, action-log)` replay. `generate_world` and
  `load_play_world` now take the projected `GenerationConfig`, and both `gen` and `play` route through it.
  A `monomyth-cli` regression test forces a room count outside the default band and asserts the `play`
  seed path no longer falls back to defaults.
- **Each `[generation]` knob is now determinism-guarded**, not just `fork_chance_permille` — per-knob
  tests in `monomyth-gen/tests/determinism.rs` assert a changed value perturbs its owning pass and leaves
  unrelated facets byte-identical, as the Confirmation section requires.
- **`discover()` is testable.** A path-injected `discover_from(deployment, user, project)` core (the
  public `discover()` supplies the real env-/CWD-derived paths) is exercised by tests for merge order, the
  missing-file skip, and a surfaced parse error, without mutating process-global state.
