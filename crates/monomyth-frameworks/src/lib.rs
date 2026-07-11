//! Comparative-mythology framework enums plus the JSON-loaded descriptions and
//! crosswalks that ground the monomyth domain model.
//!
//! This is a pure crate: no IO, no async, no rendering or generation concerns.
//! Every taxonomy in `artifacts/frameworks/*.json` is embedded at compile time
//! (`include_str!`) and exposed as a fieldless enum whose variants carry the
//! artifact's stable numeric `id`. The enums are the vocabulary the `story`
//! module and the generator build on; the JSON is the ground truth and the code
//! conforms to it (verified by the parity tests).
//!
//! # Tiers
//!
//! - **macro** — [`MonomythStage`], [`BookerPlot`], [`AtuCategory`].
//! - **meso** — [`ProppFunction`], [`PoltiSituation`], [`DundesMotifeme`].
//! - **micro** — [`MotifClass`].
//! - **character** — [`ProppRole`], [`GreimasActant`], [`Archetype`].
//! - **crosswalks** — [`arc_functions`], [`plot_situations`], [`role_alignment`].
//!
//! # Serialization
//!
//! Enums use serde's default derive, so a variant serializes as its own
//! identifier string (e.g. `"CallToAdventure"`). That representation is stable
//! and snapshot-testable; the numeric ids are an artifact detail, reachable via
//! [`MonomythStage::id`] / [`MonomythStage::from_id`] but never on the wire.

#![forbid(unsafe_code)]

mod character;
mod crosswalk;
mod info;
mod macro_tier;
mod macros;
mod meso_tier;
mod micro_tier;

#[cfg(test)]
mod tests;

pub use character::{Archetype, GreimasActant, ProppRole};
pub use crosswalk::{
    CharacterAlignment, arc_functions, arc_functions_weighted, plot_situations, role_actants,
    role_alignment, role_archetypes,
};
pub use info::{
    ArchetypeInfo, AtuCategoryInfo, BookerPlotInfo, DundesMotifemeInfo, GreimasActantInfo,
    MonomythStageInfo, MotifClassInfo, PoltiSituationInfo, ProppFunctionInfo, ProppRoleInfo,
};
pub use macro_tier::{AtuCategory, BookerPlot, MonomythStage};
pub use meso_tier::{DundesMotifeme, PoltiSituation, ProppFunction};
pub use micro_tier::MotifClass;
