---
priority: high
---

# TDD + poly workflow

- Practice red-green-refactor. Write a failing test first when the change is observable from a
  crate's public API. Favor deterministic tests: seed the RNG, assert on serialized snapshots.
- **Before every commit, run the local check set:**
  - `cargo fmt`
  - `cargo clippy --workspace --all-targets --tests -- -D warnings`
  - `cargo test --workspace`
  - `poly fmt --fix .` then `poly lint .`
  - When framework artifacts changed: the artifact validator (canonical counts + crosswalk
    referential integrity + license-ledger invariant) must pass.
- Clippy is strict (`-D warnings`); do not silence with `#[allow(...)]` unless the warning is
  genuinely wrong — and add a one-line `//` comment explaining why when you do.
- **Data is not source.** `artifacts/**` (framework JSON + generated corpus) and `corpus/raw/**`
  (cached downloads) are excluded from poly and are regenerated, not hand-edited.
- Commits use Conventional Commit prefixes (`feat:`, `fix:`, `perf:`, `chore:`, `refactor:`,
  `docs:`). Keep commits granular and focused; match the style in `git log`.
