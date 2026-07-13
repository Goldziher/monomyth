//! The long-form compose pipeline: turning a [`World`](monomyth_core::World)'s
//! narrative spine into extended prose.
//!
//! Compose sits downstream of `monomyth-gen`: where generation produces a
//! structured world with empty content slots (and, later, an LLM content pass
//! fills individual prose slots in place), compose is the pipeline that narrates
//! the *whole* spine into long-form text — outline, then draft, then revise, then
//! assemble.
//!
//! This slice implements only the first stage, **Plan**: a pure, deterministic
//! extraction of an [`Outline`] from [`World::story`](monomyth_core::World)'s
//! narrative spine. It performs no IO, no LLM calls, and no async work — it is a
//! synchronous function of its input, matching the procedural half of generation.
//! Draft, Revise, and Assemble are later slices and are not present here.

#![forbid(unsafe_code)]

pub mod error;
pub mod outline;
pub mod plan;

pub use error::ComposeError;
pub use outline::{Outline, OutlineSection};
pub use plan::plan;
