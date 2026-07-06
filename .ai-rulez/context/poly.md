---
priority: high
---

# poly

poly (polylint) is a single-binary, multi-language linter and formatter. It bundles engines (ruff
for Python, oxc for JS/TS/JSON, taplo for TOML, rumdl for Markdown) and delegates to native tools
(cargo fmt/clippy, shellcheck, shfmt) when present. Config is per-repo `poly.toml`.

## Commands

- Lint: `poly lint .`
- Check formatting (dry-run, the default): `poly fmt .`
- Apply formatting: `poly fmt --fix .`
- Apply lint autofixes: `poly lint --fix .`

## Conventions in this repo

- `poly.toml` excludes generated/data paths: `artifacts/**` (framework JSON + generated corpus),
  `corpus/raw/**` (cached downloads), `target/**`, and lockfiles. Do not lint or reformat those.
- `poly lint` exits non-zero only on error-severity findings; warnings do not fail.
- Cache dir `.polylint/` is gitignored.

Run `poly fmt --fix .` then `poly lint .` before committing.
