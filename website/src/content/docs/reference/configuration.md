---
title: Configuration
description: Layered TOML configuration for model routing and generation/synthesis/compose knobs.
---

monomyth resolves configuration from ordered, layered TOML files
(`monomyth-config`, [ADR-0015](https://github.com/Goldziher/monomyth/blob/main/adrs/0015-layered-override-configuration.md)).
Precedence, later wins:

```text
SystemDefault  <  $MONOMYTH_CONFIG_DIR/monomyth.toml  (deployment)
               <  ./monomyth.toml                      (project)
               <  ~/.config/monomyth/monomyth.toml     (user)
               <  CLI flags                            (runtime)
```

Every key is optional; an omitted key falls through to the layer below it. An unknown or misspelled
key is a hard error, not a silent no-op — `deny_unknown_fields` catches typos at parse time.

An unconfigured run resolves to the built-in system defaults and reproduces the pinned determinism
golden exactly: **a verbatim copy of the example file changes nothing.**

## Getting started

Copy `monomyth.example.toml` (repository root) to `monomyth.toml` in your working directory and
delete the keys you don't want to override:

```sh
cp monomyth.example.toml monomyth.toml
```

Secrets never live in the config file — a model is named by its `provider/model` routing string, but
the API key stays in the environment (for example `GEMINI_API_KEY`).

## Sections

| Section | Purpose |
|---|---|
| `[models]` | Per-task model routing: `content` (per-slot content fill), `synthesis` (law-synthesis distillation), `compose` (long-form composition). Overridden by the global `--model` flag (content and compose) and `synthesize law --model` (synthesis). |
| `[generation]` | Procedural generation knobs feeding the seeded, deterministic passes: `fork_chance_permille`, `beats_per_stage_min`/`max`, `rooms_min`/`max`, `items_min`/`max`, `max_extra_cast`. |
| `[synthesis]` | Law-synthesis judge-loop tuning: `max_iterations`, `per_query_top_k`, `max_grounding`. `per_query_top_k` is overridden by `synthesize law --top-k`. |
| `[compose]` | Long-form composition pipeline tuning (Plan → Draft → Revise → Assemble): `max_turns`, `grounding_top_k`, `revise_threshold`, `max_revise_iterations`. |

See `monomyth.example.toml` for the full, commented set of defaults and their meanings.

## Determinism constraint

A config value may change the RNG draws taken *within* a pass's own sub-stream, but must never change
the number or order of child-seed draws — that would silently break `(seed, action-log)` replay for
every downstream pass. This is enforced by determinism tests, not left to convention.

## Not yet configurable

`[retrieval]` and `[paths]` sections, the synthesis judge score thresholds, the extraction softmax
temperature, and a dedicated judge model are not yet exposed through `monomyth-config` — see the
[roadmap](https://github.com/Goldziher/monomyth/blob/main/docs/roadmap.md).
