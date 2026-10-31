# Contributing

## Tooling

Install [mise](https://mise.jdx.dev), then run `mise run setup`: it installs the pinned toolchain, fetches the
dependencies and does a first build. `mise tasks` lists the tasks; `mise run check` is the full gate.

## AI governance

ai-rulez always runs at its latest release through `npx -y ai-rulez@latest`.
Run `mise run governance:generate` after editing `.ai-rulez/`.
Run `mise run governance:check` to validate native OKF sources, the lock, and generated output.

Run `mise run governance:hooks:install` to install the reviewed validation hooks. Existing machine preferences are
preserved.
