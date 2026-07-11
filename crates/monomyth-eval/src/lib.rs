//! The deterministic, LLM-free benchmark-scoring harness of ADR-0023.
//!
//! `monomyth-eval` scores a predicted [`World`](monomyth_core::World) against a
//! hand-encoded, ground-truth gold [`World`] fixture — measuring the contract's
//! bidirectional claim (extraction and generation as inverse transforms,
//! ADR-0018/ADR-0002) and the scored-attribute schema (ADR-0022)
//! quantitatively instead of only checking that
//! [`World::validate`](monomyth_core::World::validate) passes.
//!
//! Everything in this crate is a pure function of its inputs: no RNG, no IO
//! beyond loading a fixture's bytes, and — critically — no LLM in the scoring
//! path, so a `Report` is exactly reproducible and can be pinned with an FNV
//! golden the same way `monomyth-gen`'s generator output is
//! (`monomyth-gen/tests/determinism.rs`).
//!
//! # Phase B3 scope
//!
//! [`Dist`], the four metrics ([`histogram_intersection`], [`cross_entropy`],
//! [`top1_accuracy`], [`kendall_tau`]) and [`DistScore`], the [`Benchmark`]
//! fixture loader, the [`Scorer`] trait / [`Report`] shape, and a concrete
//! node-alignment scorer, [`AlignmentScorer`], that matches an extracted
//! [`World`](monomyth_core::World)'s narrative spine against a gold fixture's via
//! Needleman-Wunsch global alignment (see [`AlignmentScorer`]'s doc for the
//! algorithm and its scope limitation).

#![forbid(unsafe_code)]

mod alignment;
mod benchmark;
mod dist;
mod export;
mod metrics;
mod report;
pub mod util;

pub use alignment::AlignmentScorer;
pub use benchmark::{Benchmark, BenchmarkError};
pub use dist::Dist;
pub use export::{TrainingExample, stage_training_examples, to_jsonl};
pub use metrics::{DistScore, cross_entropy, histogram_intersection, kendall_tau, top1_accuracy};
pub use report::{Alignment, Report, Scorer, report_fingerprint};
