---
title: Installation
description: Build the monomyth workspace from source — prerequisites, setup, and the task runner.
---

monomyth is a Rust workspace, not a published package. Installing it means cloning the repository
and building it.

## Prerequisites

- **Rust**, pinned via [`rust-toolchain.toml`](https://github.com/Goldziher/monomyth/blob/main/rust-toolchain.toml)
  at the repository root. `rustup` picks up the pin automatically (channel `1.96`, with the
  `rustfmt` and `clippy` components) — no manual toolchain selection needed.
- **[`mise`](https://mise.jdx.dev)** — installs the pinned tools and runs the build/test/lint tasks.
  Optional but recommended; see [`mise.toml`](https://github.com/Goldziher/monomyth/blob/main/mise.toml).
- **[`poly`](https://github.com/Goldziher/poly)** (polylint) — formats and lints the non-Rust files
  in the repo (Markdown, TOML). Only needed if you plan to contribute changes; `mise install` provides it.

## Clone and build

```sh
git clone https://github.com/Goldziher/monomyth.git
cd monomyth
mise run setup
```

`mise run setup` installs the pinned tools, fetches dependencies and does a first debug build of the whole
workspace (`mise install && cargo fetch && cargo build --workspace`). Without mise, run the equivalent `cargo` commands
directly.

List every available task with:

```sh
mise tasks
```

The commands that matter day to day:

| Task | What it does |
|---|---|
| `mise run setup` | Install the pinned tools, fetch dependencies, first build. |
| `mise run build` | Build the whole workspace. |
| `mise run test` | Run the full test suite (`cargo test --workspace`). |
| `mise run format` | Apply Rust and poly formatting in place. |
| `mise run lint` | Clippy (deny warnings) plus poly lint. |
| `mise run bench` | Run the Criterion microbenchmarks. |
| `mise run check` | The full pre-commit gate — format check, lint, and tests. |

## Running the CLI

Once built, invoke subcommands through `cargo run -p monomyth-cli --`, for example:

```sh
cargo run -p monomyth-cli -- gen --seed 42
```

See the [Quickstart](/monomyth/start/quickstart/) for a full walkthrough, and the
[CLI reference](/monomyth/reference/cli/) for every subcommand.

## LLM-backed features

Some subcommands (`gen --fill`, `compose`, `synthesize law`) call an LLM through
[`monomyth-llm`](/monomyth/reference/crates/) and need a configured provider. Copy
`monomyth.example.toml` to `monomyth.toml` and set a `provider/model` routing string under
`[models]`; the corresponding API key is read from the environment (for example `GEMINI_API_KEY`),
never from the config file. See [Configuration](/monomyth/reference/configuration/).
