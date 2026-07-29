---
title: CLI
description: Every monomyth subcommand — gen, play, edit, ingest, retrieve, corpus, compose, eval, finetune-export, synthesize, and extract.
---

Invoke the binary through `cargo run -p monomyth-cli -- <subcommand>` from the workspace root. The
CLI is a harness over the engine, not the product itself.

## Global flags

| Flag | Purpose |
|---|---|
| `--model <provider/model>` | Routing string for the LLM content pass (`gen --fill`) and `compose`. Overrides the configured `models.content` / `models.compose` default. |
| `--db <path>` | Path to the knowledge vector store used by `ingest`, `retrieve`, and `--fill`. Defaults to `./monomyth.db`. |

## Subcommands

| Command | Purpose |
|---|---|
| `gen --seed <u64> [--fill] [--out <path>]` | Generate a world from a seed, render it, and emit its serialized form. `--fill` also runs the non-deterministic LLM content pass. |
| `play [--world <path>] [--seed <u64>]` | Play a world interactively: load one from disk or generate from a seed. |
| `edit --world <path> --script <path> [--out <path>]` | Apply a JSON edit script (a list of `NarrativeEdit` operations) to a world's narrative structure and re-validate transactionally. |
| `ingest <source> [--text <str> \| --file <path>] [--reference] [--title <str>] [--url <str>]` | Ingest a declared source's text into the knowledge store. Ship-namespace by default; `--reference` ingests into the priors-only reference collection instead. The ingest gate refuses a source whose ledger namespace does not match the chosen path. |
| `retrieve <query> [--top-k <u32>]` | Retrieve surfaceable passages matching a query. Defaults to 5 results. |
| `corpus <build\|audit\|inspect>` | Manage the ship-safe corpus. See below. |
| `eval --work <id> [--extractor <str>] [--benchmarks-dir <path>] [--out <path>]` | Score an extractor against a ground-truth benchmark fixture. |
| `finetune-export --work <id> [--benchmarks-dir <path>] [--out <path>]` | Export PD-gated text↔scored-structure fine-tune pairs for a fixture, as JSONL. |
| `synthesize <law\|promote>` | Draft or promote a structural law artifact. See below. |
| `compose --seed <u64> [--out <path>] [--json]` | Compose long-form adventure prose for a seeded world through Plan → Draft → Revise → Assemble. `--json` emits the full `LongFormDoc` (sections + prose) instead of just the text. |
| `extract --input <path> [--out <path>] [--genre <str>]` | Derive a minimal world structure from raw source text via a single structured LLM call. `--genre` biases the extraction prompt (`myth`, `detective`, `litrpg`; unrecognized names fall back to `myth`). |

### `corpus` subcommands

| Command | Purpose |
|---|---|
| `corpus build [--source <id>] [--limit <n>]` | Fetch, normalize, and ingest ship-safe sources declared in the ledger. |
| `corpus audit` | Audit stored document metadata against the license ledger — the third of the three licensing enforcement points, after ingest and retrieval. |
| `corpus inspect [--source <id>] [--limit <n>]` | Download reference/unverified sources into the `reference/` inspect area for licensing review. Never ingested into the ship corpus. |

### `synthesize` subcommands

| Command | Purpose |
|---|---|
| `synthesize law --law <id> --domain <str> --query <str> [--top-k <n>] [--model <str>] [--coverage-framework <str>] [--out <path>]` | Draft one candidate law from reference-namespace priors and write it to `synthesis/candidates/` for human review. `--coverage-framework` (e.g. `campbell`) seeds coverage sub-queries spanning a taxonomy's full stage set. |
| `synthesize promote <candidate> --reviewed-by <str> [--laws-dir <path>]` | Promote a human-reviewed candidate into `artifacts/laws/`. Stamps `reviewed_by`, re-runs the anti-leak gate, validates the result, and refuses a law id that is already promoted. |

## Requires provider configuration

`gen --fill`, `compose`, and `synthesize law` call an LLM and need a `[models]` entry resolved
through [Configuration](/monomyth/reference/configuration/) (or the `--model` flag), plus the
corresponding API key in the environment.
