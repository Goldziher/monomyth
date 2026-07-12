//! Typed structured LLM generation over the xberg core, with a swappable backend
//! seam for deterministic testing.
//!
//! The crate's reason to exist is [`Llm::generate`], a generic over any
//! `serde::de::DeserializeOwned + schemars::JsonSchema` type that returns a
//! validated Rust value. xberg's structured helper builds its client internally
//! and is not injectable, so this crate defines its own [`StructuredBackend`]
//! trait: production uses [`XbergBackend`] (which calls the xberg free
//! functions); tests inject a fake.
//!
//! ```no_run
//! use monomyth_llm::Llm;
//! use schemars::JsonSchema;
//! use serde::Deserialize;
//!
//! #[derive(Debug, Deserialize, JsonSchema)]
//! struct Omen {
//!     text: String,
//! }
//!
//! # async fn run() -> Result<(), monomyth_llm::LlmError> {
//! let llm = Llm::from_env("anthropic/claude-sonnet-4-20250514")?;
//! let omen = llm.generate::<Omen>("Foretell the hero's departure.", "omen").await?;
//! println!("{}", omen.value.text);
//! # Ok(())
//! # }
//! ```

#![forbid(unsafe_code)]

mod backend;
mod cassette;
mod error;
mod llm;

pub use backend::{BackendOptions, StructuredBackend, Usage, XbergBackend};
pub use cassette::{Cassette, Interaction, RecordingBackend, ReplayBackend};
pub use error::{BackendError, CassetteError, LlmError};
pub use llm::{Generated, Llm, MAX_ATTEMPTS};
