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
//! the building block the Draft phase calls once per outline section to narrate
//! it. **Draft** ([`draft`]) orchestrates that primitive across a whole
//! [`Outline`], threading prior prose forward for cross-section coherence, and
//! scores/revises each section against its grounding (see [`revise`]).
//! **Assemble** ([`assemble`]) stitches the drafted sections into one
//! [`LongFormDoc`]. [`compose`] wires Plan -> Draft -> Assemble into a single
//! end-to-end entry point. Any live/recorded cassette is a later slice and is
//! not present here.

#![forbid(unsafe_code)]

pub mod assemble;
pub mod draft;
pub mod error;
pub mod generate;
pub mod outline;
pub mod plan;
pub mod revise;
pub mod settings;

#[cfg(test)]
mod test_support;

pub use assemble::{LongFormDoc, SectionDraft, assemble};
pub use draft::{compose, draft};
pub use error::ComposeError;
pub use generate::{Continuation, DEFAULT_MAX_TURNS, generate_long_form};
pub use outline::{Outline, OutlineSection};
pub use plan::plan;
pub use settings::ComposeSettings;
