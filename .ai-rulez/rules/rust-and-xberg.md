---
priority: high
---

# Rust & xberg conventions

- **Edition 2024**, a Cargo workspace under `crates/`. The root is a virtual manifest
  (`[workspace]` only). Keep the domain model (`monomyth-core`) free of IO, rendering, and
  generation concerns — it is the serializable contract every other crate shares.
- **Serialization:** `serde` derives on every domain type; deterministic collections (`BTreeMap` /
  `BTreeSet`, not `Hash*`) so serialized output is stable and snapshot-testable. Stable, generational,
  typed IDs (slotmap keys), never `Vec` indices, for anything referenced across the model.
- **Determinism:** the procedural half of generation is a pure function of a seed
  (`rand_chacha` sub-streams). The LLM/content half is quarantined and never touches the
  procedural RNG. A play session must replay from `(seed, action-log)`.
- **xberg RAG** (see corpus-and-rag context): consume `xberg-rag` with features
  `vector-store` + `sqlite` + `pipeline` + `pipeline-embeddings`. Depend only on
  `Arc<dyn VectorStore>` in engine code so the backend (sqlite-vec now, LanceDB/pgvector later)
  stays swappable. To add a backend, implement the 9-method `VectorStore` trait in a feature-gated
  adapter module — do not fork xberg.
- **Errors:** `thiserror` for library error enums; no `unwrap()`/`expect()` in non-test code paths
  that can fail on input. `#![forbid(unsafe_code)]` in the pure crates.
- **Feature flags** gate optional backends and heavy deps (ONNX, LanceDB, pgvector). Default features
  stay minimal so a consumer pulls in only what it uses.
