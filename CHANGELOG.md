# Changelog

All notable changes to monomyth are recorded here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project aims to follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html). Pre-1.0, the public API and the
serialized world/story schema may change between releases.

## [Unreleased]

### Added

- **Layered configuration** (`monomyth-config`): a TOML override resolver with per-task model routing
  and generation / synthesis / compose knobs. An unconfigured run reproduces the built-in defaults
  bit-for-bit, so the determinism golden never drifts (ADR-0015).
- **Evaluation harness** (`monomyth-eval`): a deterministic, LLM-free benchmark scorer over the
  contract's classification/distribution claims, an optional embedding-cosine semantic axis, and
  performance baselines (ADR-0023).
- **Long-form compose pipeline** (`monomyth-compose`): Plan → Draft → Revise → Assemble over a
  finished `World`, grounded on reference-namespace priors and quarantined from the procedural RNG;
  returns a sidecar `LongFormDoc`, never mutating the world (ADR-0027).
- **Build-time law synthesis** (`monomyth-synthesis`): a judge-gated pipeline that drafts abstract
  taxonomies from reference-namespace priors for human review, never surfacing prose (ADR-0016,
  ADR-0024).
- **Reference-path RAG enrichment** (`monomyth-knowledge`): best-effort YAKE/RAKE keyword and NER
  extraction at ingest, used to sharpen reference-path retrieval. The surfaceable ship path stays
  byte-identical (ADR-0026).
- **Extraction proof-of-concept** (`monomyth-extract`): a hermetic, LLM-free RAG-softmax extractor
  turning text back into the structured model (ADR-0018).
- CLI subcommands: `compose`, `eval`, `finetune-export`, and `synthesize law`.
- A `Taskfile.yaml` wrapping the cargo + poly gate set.

### Changed

- The RAG base layer (formerly the standalone `xberg-rag` crate) now lives in-tree as
  `monomyth-knowledge`'s `rag` submodule, consuming published `xberg` from crates.io — removing the
  last relative-path dependency.
- Procedural generation bounds (fork chance, beats per stage, room/item/cast counts) are now resolved
  through `monomyth-config` rather than hardcoded constants; defaults are unchanged.

[Unreleased]: https://github.com/Goldziher/monomyth/commits/main
