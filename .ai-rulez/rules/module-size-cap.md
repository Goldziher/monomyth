---
priority: normal
---

# Keep modules focused

- Prefer small, single-responsibility modules. When a Rust file approaches ~1000 lines, refactor by
  extracting helpers, types, or submodules rather than letting it grow.
- Split by concern, not arbitrarily: e.g. a domain aggregate, its operations, and its queries are
  natural module boundaries. Thin public surfaces should delegate to focused helper modules.
- The same discipline applies to the framework artifacts: one framework or crosswalk per JSON file,
  each validated independently.
