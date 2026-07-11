---
priority: high
---

# Minimal public surface, documented

- **`pub(crate)` by default.** Expose the smallest surface that callers need; prefer `pub(crate)` for
  anything internal to a crate. A type or function becomes `pub` only when it is genuinely part of
  the crate's API contract.
- **Document what is public.** Every genuinely-`pub` item (type, function, constant, trait) carries a
  rustdoc comment explaining *why* it exists, not what the code does. Public examples use `?`, not
  `.unwrap()`. Document errors, panics, and safety invariants.
- **Newtypes at boundaries.** Prefer newtypes over bare primitives for public parameters and IDs
  (the workspace already uses typed slotmap keys); avoid `bool` parameters — use enums.
- **Keep struct fields private** unless the type is a plain serializable data record (the domain
  contract's `serde` types are the deliberate exception). Delegate thin public surfaces to focused
  helper modules (see the module-size-cap rule).
