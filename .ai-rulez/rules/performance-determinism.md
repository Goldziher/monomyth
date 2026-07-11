---
priority: high
---

# Performance patterns, determinism-first

The deterministic-serialization contract dominates every performance choice: serialized output must
be byte-stable and snapshot-testable, so the domain model uses `BTreeMap`/`BTreeSet`, and a play
session replays from `(seed, action-log)`. Optimize within that constraint, never against it.

- **Never** replace `BTreeMap`/`BTreeSet` with `HashMap`/`HashSet` in or feeding serialized output —
  iteration order *is* the deterministic contract. `ahash`/`foldhash` are **non-patterns** here
  (they are transitive deps only); use a faster hasher **only** for an internal, non-serialized,
  order-irrelevant hot map, and add a `//` comment justifying it.
- **`memchr`** for byte/substring scanning over large corpus text (already in `Cargo.lock`). The
  concrete win today is single-pass CRLF→LF normalization in `acquire/normalize.rs` (multi-MB books);
  reach for `memchr`/`memchr::memmem` before hand-rolled char-by-char `.replace()` loops on big text.
- **Compile regexes once** in `LazyLock` statics — never per call. Same for parsed framework metadata.
- **Bounded streaming** for network bodies — never buffer an unbounded response (the acquire HTTP
  path caps at 256 MB and streams in chunks). **Preallocate** `String`/`Vec` when the size is known.
- **Don't hand-roll SIMD / vector math** — embedding and similarity math live in xberg/ONNX, which
  owns those hot loops; it is not our code path.
- Config values must be **determinism-safe**: they may change draws *within* a pass's RNG sub-stream
  but must never change the number or order of child-seed draws (see ADR-0015).
