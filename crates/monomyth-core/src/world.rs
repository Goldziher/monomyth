//! The world aggregate: [`World`] plus its [`Location`] graph and bookkeeping.
//!
//! [`World`] is the serializable contract every other crate shares — the only
//! thing that crosses between generation and a frontend. It holds each element
//! kind in its own [`SlotMap`] (keyed by the typed ids in [`crate::ids`]) so
//! cross-references stay stable across edits and serialization. Auxiliary
//! relations use [`BTreeMap`]/[`BTreeSet`] for deterministic, snapshot-testable
//! output.
//!
//! [`World`] and [`Story`](crate::Story) do not implement [`PartialEq`] because
//! [`SlotMap`] does not; compare two worlds by their serialized form, which is
//! exactly the equality the determinism guarantees cover.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

use crate::content::Content;
use crate::entity::{Entity, Item, Player};
use crate::ids::{EntityId, ItemId, LocationId, NarrativeNodeId};
use crate::rng::RngState;
use crate::story::Story;

/// The serialization schema version stamped into every [`WorldMeta`].
///
/// A named constant so a loader can reject or migrate an incompatible world
/// rather than misinterpreting its fields.
///
/// Bumped 1 → 2 for the breaking narrative-structure change: the flat `Story.arc`
/// / `current_stage` fields were replaced by a branching
/// [`NarrativeStructure`](crate::NarrativeStructure) and a
/// [`WorldState::cursor`] play position.
pub const SCHEMA_VERSION: u32 = 2;

/// A compass or vertical direction connecting two locations.
///
/// [`Copy`] + [`Ord`] so it can key a [`BTreeMap`] of exits with deterministic
/// iteration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Direction {
    /// Northward exit.
    North,
    /// Southward exit.
    South,
    /// Eastward exit.
    East,
    /// Westward exit.
    West,
    /// Upward exit.
    Up,
    /// Downward exit.
    Down,
}

impl Direction {
    /// The opposing direction, for wiring reciprocal exits between locations.
    ///
    /// ```
    /// use monomyth_core::Direction;
    ///
    /// assert_eq!(Direction::North.opposite(), Direction::South);
    /// assert_eq!(Direction::Up.opposite(), Direction::Down);
    /// ```
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::North => Self::South,
            Self::South => Self::North,
            Self::East => Self::West,
            Self::West => Self::East,
            Self::Up => Self::Down,
            Self::Down => Self::Up,
        }
    }
}

/// Identity and versioning for a generated world.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldMeta {
    /// The procedural seed the world was generated from.
    pub seed: u64,
    /// The schema version the world was serialized under (see [`SCHEMA_VERSION`]).
    pub schema_version: u32,
    /// The world's title slot.
    pub title: Content,
}

/// Mutable play-session bookkeeping that is not part of the generated structure.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldState {
    /// The number of successfully applied actions so far.
    pub turn: u64,
    /// Named boolean flags set by gameplay (quest triggers, doors, …).
    pub flags: BTreeMap<String, bool>,
    /// The player's current position in the
    /// [`Story::structure`](crate::Story::structure), starting at its
    /// [`root`](crate::NarrativeStructure::root).
    ///
    /// [`Default`] is a null [`NarrativeNodeId`]; generation and any bootstrap must
    /// set it to the structure's root before play.
    pub cursor: NarrativeNodeId,
}

/// A node in the world graph: its prose, its exits, and what occupies it.
///
/// Occupancy ([`entities`](Self::entities), [`items`](Self::items)) is stored as
/// [`BTreeSet`]s of ids — the elements themselves live in the [`World`]'s
/// slotmaps, so a location references them rather than owning them.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Location {
    /// The location's name slot.
    pub name: Content,
    /// The location's description slot.
    pub description: Content,
    /// Exits from this location, keyed by direction.
    pub exits: BTreeMap<Direction, LocationId>,
    /// Ids of entities currently here.
    ///
    /// This is one side of the entity/location relationship whose other side is
    /// [`Entity::location`](crate::Entity::location); the two must be kept in sync
    /// by the caller. The engine does not move entities yet, so nothing maintains
    /// this invariant automatically.
    pub entities: BTreeSet<EntityId>,
    /// Ids of items currently here.
    pub items: BTreeSet<ItemId>,
}

/// The complete, serializable state of a playable world.
///
/// This is the shared contract: generation produces it, the engine mutates it
/// through [`apply`](crate::apply), and a frontend renders it — none of them see
/// anything else across the boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct World {
    /// Identity and versioning.
    pub meta: WorldMeta,
    /// All locations, keyed by [`LocationId`].
    pub locations: SlotMap<LocationId, Location>,
    /// All entities, keyed by [`EntityId`].
    pub entities: SlotMap<EntityId, Entity>,
    /// All items, keyed by [`ItemId`].
    pub items: SlotMap<ItemId, Item>,
    /// The player avatar.
    pub player: Player,
    /// The story spine and quests.
    pub story: Story,
    /// Turn counter and gameplay flags.
    pub state: WorldState,
    /// The deterministic procedural RNG stream.
    pub rng: RngState,
}
