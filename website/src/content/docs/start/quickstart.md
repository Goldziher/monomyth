---
title: Quickstart
description: Generate a world, play it, edit its narrative structure, and compose long-form prose from the CLI.
---

This walks through the `monomyth` CLI end to end: generating a deterministic world, playing it,
editing its narrative structure, filling it with grounded prose, and building the corpus that grounds
it. All commands assume you have [built the workspace](/monomyth/start/installation/).

## Generate a world

```sh
cargo run -p monomyth-cli -- gen --seed 42
```

This runs the deterministic procedural pass and renders the branching narrative structure it
produces. The same seed always reproduces a byte-identical world.

Write the serialized world to a file, then walk its forks interactively:

```sh
cargo run -p monomyth-cli -- gen --seed 42 --out world.json
cargo run -p monomyth-cli -- play --world world.json
```

## Edit a narrative structure

Apply a JSON script of narrative edit operations to a world; the result is re-validated
transactionally — an invalid edit rolls the whole batch back:

```sh
cargo run -p monomyth-cli -- edit --world world.json --script edits.json
```

## Fill content from the corpus

Generation alone leaves prose slots empty. Fill them from the ship-gated corpus via the LLM (requires
[provider configuration](/monomyth/reference/configuration/)):

```sh
cargo run -p monomyth-cli -- gen --seed 42 --fill
```

## Compose long-form prose

Turn a finished world into a multi-paragraph adventure via the Plan → Draft → Revise → Assemble
pipeline (also requires provider configuration):

```sh
cargo run -p monomyth-cli -- compose --seed 42 --out adventure.md
```

## Evaluate and export

Score an extractor against a ground-truth benchmark fixture, or export PD-gated fine-tune pairs:

```sh
cargo run -p monomyth-cli -- eval --work odyssey_campbell_macro
cargo run -p monomyth-cli -- finetune-export --work odyssey_campbell_macro --out pairs.jsonl
```

## Draft a structural law

Draft a pre-review candidate "law" — an abstract taxonomy — from reference-namespace priors:

```sh
cargo run -p monomyth-cli -- synthesize law --law three_act --domain myth --query "story structure"
```

The result is a candidate written for human review, never a ship-safe artifact until promoted. See
[Corpus & licensing](/monomyth/concepts/corpus-licensing/).

## Build the corpus

Build, or audit against the license ledger, the ship-safe corpus declared in the manifest:

```sh
cargo run -p monomyth-cli -- corpus build
cargo run -p monomyth-cli -- corpus audit
```

## Next steps

- [CLI reference](/monomyth/reference/cli/) — every subcommand and flag.
- [Hybrid generation](/monomyth/concepts/hybrid-generation/) — how the procedural and LLM passes stay
  separated.
- [The narrative graph](/monomyth/concepts/narrative-graph/) — what `--script edits.json` actually
  mutates.
