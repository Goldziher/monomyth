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
//! # Phase B1 scope
//!
//! This is the scaffold: [`Dist`], the four metrics ([`histogram_intersection`],
//! [`cross_entropy`], [`top1_accuracy`], [`kendall_tau`]) and [`DistScore`], the
//! [`Benchmark`] fixture loader, and the [`Scorer`] trait / [`Report`] skeleton.
//! Node alignment (matching extracted narrative nodes to gold nodes) is Phase
//! B3; the first real fixture under `artifacts/benchmarks/` is Phase B2. Neither
//! exists yet — [`Report::alignment`] is always empty, and [`Benchmark::load`]
//! has no real fixture to load from.

#![forbid(unsafe_code)]

mod benchmark;
mod dist;
mod metrics;
mod report;
pub mod util;

pub use benchmark::{Benchmark, BenchmarkError};
pub use dist::Dist;
pub use metrics::{DistScore, cross_entropy, histogram_intersection, kendall_tau, top1_accuracy};
pub use report::{Alignment, Report, Scorer, report_fingerprint};
