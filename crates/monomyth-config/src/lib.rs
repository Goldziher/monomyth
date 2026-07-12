//! Layered TOML configuration for the monomyth workspace (ADR-0015).
//!
//! Every tunable that was a hardcoded `const` is migrated here as a
//! [`Layered<T>`] value resolved from five override layers —
//! [`SystemDefault`](LayerSource::SystemDefault) (the compiled default, equal to
//! the old constant) < [`DeploymentDefault`](LayerSource::DeploymentDefault) <
//! [`ProjectOverride`](LayerSource::ProjectOverride) (`./monomyth.toml`) <
//! [`UserOverride`](LayerSource::UserOverride) < [`RuntimeOverride`](LayerSource::RuntimeOverride)
//! (CLI flags). An *unconfigured* run resolves to exactly the old constants, so
//! existing behaviour — including `monomyth-gen`'s determinism golden — never
//! drifts.
//!
//! This crate is a leaf: it depends only on `serde`/`toml`/`dirs`/`thiserror` and
//! is depended on by the CLI and generation crates, never the reverse. It holds no
//! secrets — a config value names a `provider/model`, but the API key stays in the
//! environment (`.env`) and is resolved elsewhere.
//!
//! # Example
//!
//! ```
//! use monomyth_config::ConfigResolver;
//!
//! # fn main() -> Result<(), monomyth_config::ConfigError> {
//! // The deterministic, filesystem-free entry point (used by tests).
//! let config = ConfigResolver::defaults().resolve()?;
//! assert_eq!(*config.generation.fork_chance_permille.get(), 500);
//! # Ok(())
//! # }
//! ```
//!
//! Carried today: the `[generation]` (fork probability + beat/map/item/cast bounds),
//! `[models]` (per-task routing), and `[synthesis]` (judge-loop knobs) sections.
//! Later slices add the `[retrieval]`, `[paths]`, and `[knowledge]` sections the
//! same way.

#![forbid(unsafe_code)]

mod error;
mod layered;
mod resolver;
mod schema;

pub use error::ConfigError;
pub use layered::{LayerSource, Layered};
pub use resolver::{ConfigResolver, RuntimeOverrides};
pub use schema::{
    GenerationSection, GenerationSettings, ModelRole, ModelsSection, ModelsSettings,
    MonomythConfig, MonomythConfigFile, SynthesisSection, SynthesisSettings,
};
