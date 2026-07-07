//! The concrete procedural passes that make up the default pipeline.
//!
//! Each submodule implements one [`ProceduralPass`](crate::ProceduralPass) with a
//! single responsibility: [`BackbonePass`] builds the branching narrative
//! structure, [`BeatPass`] grows it to a playable length, [`MapPass`] the location
//! graph, [`CastPass`] the entities, and [`ItemsPass`] the items.

mod backbone;
mod beat;
mod cast;
mod items;
mod map;

pub use backbone::{BackbonePass, NarrativeConfig, PlotChoice};
pub use beat::{BeatConfig, BeatPass};
pub use cast::CastPass;
pub use items::ItemsPass;
pub use map::{MAX_ROOMS, MIN_ROOMS, MapPass};
