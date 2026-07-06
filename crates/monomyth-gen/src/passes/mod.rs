//! The concrete procedural passes that make up the default pipeline.
//!
//! Each submodule implements one [`ProceduralPass`](crate::ProceduralPass) with a
//! single responsibility: [`MapPass`] builds the location graph, [`ArcPass`] the
//! story spine, [`CastPass`] the entities, and [`ItemsPass`] the items.

mod arc;
mod cast;
mod items;
mod map;

pub use arc::ArcPass;
pub use cast::CastPass;
pub use items::ItemsPass;
pub use map::{MAX_ROOMS, MIN_ROOMS, MapPass};
