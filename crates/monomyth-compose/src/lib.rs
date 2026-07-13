//! The long-form compose pipeline: turning a [`World`](monomyth_core::World)'s
//! narrative spine into extended prose.
//!
//! Compose sits downstream of `monomyth-gen`: where generation produces a
//! structured world with empty content slots (and, later, an LLM content pass
//! fills individual prose slots in place), compose is the pipeline that narrates
//! the *whole* spine into long-form text — outline, then draft, then revise, then
//! assemble.
//!
//! The first stage, **Plan**, is a pure, deterministic extraction of an
//! [`Outline`] from [`World::story`](monomyth_core::World)'s narrative spine.
//! It performs no IO, no LLM calls, and no async work — it is a synchronous
//! function of its input, matching the procedural half of generation.
//!
//! [`generate_long_form`] is the multi-turn long-form generation *primitive*:
//! the building block the (future) Draft phase will call once per outline
//! section to narrate it. This slice ships the primitive alone, tested against
//! a fake in-memory backend — Draft-phase orchestration, retrieval grounding,
//! the revise loop, and any live/recorded cassette are later slices and are
//! not present here.

#![forbid(unsafe_code)]

pub mod error;
pub mod generate;
pub mod outline;
pub mod plan;

pub use error::ComposeError;
pub use generate::{Continuation, DEFAULT_MAX_TURNS, generate_long_form};
pub use outline::{Outline, OutlineSection};
pub use plan::plan;
