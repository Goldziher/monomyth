//! Whole-[`World`] referential-integrity checks and a schema-guarded load.
//!
//! The engine and every frontend trust that a [`World`]'s cross-references line
//! up: the player stands in a real location, occupancy is mirrored on both sides
//! of the entity/location relationship, and the play cursor points at a real
//! narrative beat. Generation is supposed to maintain those invariants, but
//! nothing in the type system enforces them and a deserialized world is external
//! input. [`World::validate`] is the single gate that turns those latent
//! assumptions into checked ones, and [`World::from_json_checked`] wraps
//! deserialization so a mismatched schema or a corrupt world is rejected at the
//! boundary rather than misbehaving deep in the engine.
//!
//! Like [`NarrativeStructure::validate`](crate::NarrativeStructure::validate), the
//! checks iterate slotmap keys in sorted order so the smallest offending id is
//! reported first, keeping failures snapshot-stable.

use crate::ids::{EntityId, ItemId, LocationId, NarrativeNodeId};
use crate::narrative::NarrativeError;
use crate::world::{SCHEMA_VERSION, World};
use thiserror::Error;

/// Why a [`World`] is not internally consistent.
///
/// Id-carrying variants name the offending element; [`World::validate`] iterates
/// slotmap keys in sorted order so the smallest offending id is reported first,
/// keeping failures snapshot-stable. Every variant's payload is [`Copy`], and the
/// wrapped [`NarrativeError`] is `Copy`/`Hash`, so this enum derives them too.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Error)]
pub enum WorldError {
    /// [`player.location`](crate::Player::location) is not a key in
    /// [`World::locations`](crate::World).
    #[error("player location {0:?} is not a known location")]
    DanglingPlayerLocation(LocationId),
    /// An id in [`player.inventory`](crate::Player::inventory) is not a key in
    /// [`World::items`](crate::World).
    #[error("player inventory item {0:?} is not a known item")]
    DanglingInventoryItem(ItemId),
    /// The two sides of the entity/location relationship disagree: an entity's
    /// [`location`](crate::Entity::location) is not mirrored in that location's
    /// [`entities`](crate::Location::entities), or a location lists an entity whose
    /// own `location` does not point back.
    #[error("entity {entity:?} and its location disagree about occupancy")]
    EntityLocationDesync {
        /// The entity whose occupancy is out of sync.
        entity: EntityId,
    },
    /// A [`location.entities`](crate::Location::entities) id is not a key in
    /// [`World::entities`](crate::World).
    #[error("location {location:?} lists entity {entity:?} which does not exist")]
    DanglingLocationEntity {
        /// The location holding the offending reference.
        location: LocationId,
        /// The non-existent entity it lists.
        entity: EntityId,
    },
    /// An [`entity.location`](crate::Entity::location) points at a location that is
    /// not a key in [`World::locations`](crate::World).
    #[error("entity {entity:?} claims location {location:?} which does not exist")]
    DanglingEntityLocation {
        /// The entity holding the offending reference.
        entity: EntityId,
        /// The non-existent location it claims.
        location: LocationId,
    },
    /// A [`location.items`](crate::Location::items) id is not a key in
    /// [`World::items`](crate::World).
    #[error("location {location:?} lists item {item:?} which does not exist")]
    DanglingLocationItem {
        /// The location holding the offending reference.
        location: LocationId,
        /// The non-existent item it lists.
        item: ItemId,
    },
    /// [`state.cursor`](crate::WorldState::cursor) is not a node in
    /// [`story.structure`](crate::Story::structure).
    #[error("cursor {0:?} is not a node in the narrative structure")]
    CursorNotInStructure(NarrativeNodeId),
    /// The narrative DAG itself is malformed (see [`NarrativeError`]).
    #[error(transparent)]
    Narrative(#[from] NarrativeError),
}

/// Why loading a serialized [`World`] failed.
///
/// Wraps [`serde_json::Error`], which is neither `Clone` nor `PartialEq`, so this
/// enum derives only [`Debug`] (unlike [`WorldError`]).
#[derive(Debug, Error)]
pub enum LoadError {
    /// The serialized world's schema version does not match this engine's
    /// [`SCHEMA_VERSION`].
    #[error("world schema version {found} does not match engine {expected}")]
    SchemaMismatch {
        /// The version stamped into the serialized world.
        found: u32,
        /// The version this engine understands.
        expected: u32,
    },
    /// The JSON could not be deserialized into a [`World`].
    #[error(transparent)]
    Deserialize(#[from] serde_json::Error),
    /// The world deserialized but failed [`World::validate`].
    #[error(transparent)]
    Invalid(#[from] WorldError),
}

impl World {
    /// Slotmap keys of `map` in ascending order, for snapshot-stable iteration.
    fn sorted_keys<K, V>(map: &slotmap::SlotMap<K, V>) -> Vec<K>
    where
        K: slotmap::Key + Ord,
    {
        let mut keys: Vec<K> = map.keys().collect();
        keys.sort_unstable();
        keys
    }

    /// Check every whole-world referential-integrity invariant.
    ///
    /// Enforces, in this fixed order: the narrative DAG is well-formed
    /// ([`structure.validate`](crate::NarrativeStructure::validate)); the play cursor
    /// is a real node; the player's location and every carried item exist; each
    /// location's listed entities and items exist; each placed entity's location
    /// exists and mirrors the entity back; and each location's listed entities point
    /// back at it. Slotmap keys are iterated in sorted order, so the smallest
    /// offending id is reported first.
    ///
    /// # Errors
    ///
    /// Returns the [`WorldError`] naming the first invariant violated and the
    /// smallest offending id.
    ///
    /// ```
    /// use monomyth_core::{LocationId, WorldError};
    /// use monomyth_core::doc_support::single_room_world;
    ///
    /// let world = single_room_world();
    /// world.validate()?;
    ///
    /// let mut broken = single_room_world();
    /// broken.player.location = LocationId::default();
    /// assert_eq!(broken.validate(), Err(WorldError::DanglingPlayerLocation(LocationId::default())));
    /// # Ok::<(), WorldError>(())
    /// ```
    pub fn validate(&self) -> Result<(), WorldError> {
        // 1. The narrative DAG (converts via `#[from]`).
        self.story.structure.validate()?;

        if !self.story.structure.nodes.contains_key(self.state.cursor) {
            return Err(WorldError::CursorNotInStructure(self.state.cursor));
        }

        if !self.locations.contains_key(self.player.location) {
            return Err(WorldError::DanglingPlayerLocation(self.player.location));
        }
        for &item in &self.player.inventory {
            if !self.items.contains_key(item) {
                return Err(WorldError::DanglingInventoryItem(item));
            }
        }

        for location in Self::sorted_keys(&self.locations) {
            let cell = &self.locations[location];
            for &entity in &cell.entities {
                if !self.entities.contains_key(entity) {
                    return Err(WorldError::DanglingLocationEntity { location, entity });
                }
            }
            for &item in &cell.items {
                if !self.items.contains_key(item) {
                    return Err(WorldError::DanglingLocationItem { location, item });
                }
            }
        }

        for entity in Self::sorted_keys(&self.entities) {
            let Some(location) = self.entities[entity].location else {
                continue;
            };
            let Some(cell) = self.locations.get(location) else {
                return Err(WorldError::DanglingEntityLocation { entity, location });
            };
            if !cell.entities.contains(&entity) {
                return Err(WorldError::EntityLocationDesync { entity });
            }
        }

        for location in Self::sorted_keys(&self.locations) {
            for &entity in &self.locations[location].entities {
                if self.entities[entity].location != Some(location) {
                    return Err(WorldError::EntityLocationDesync { entity });
                }
            }
        }

        Ok(())
    }

    /// Deserialize a [`World`] from JSON, then reject it unless its schema matches
    /// this engine and it passes [`validate`](Self::validate).
    ///
    /// This is the load boundary: serialized worlds are external input, so a
    /// version skew or a corrupt cross-reference is caught here rather than
    /// surfacing as a panic or a wrong render later.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError::Deserialize`] if the JSON is not a well-formed world,
    /// [`LoadError::SchemaMismatch`] if its [`schema_version`](crate::WorldMeta) is
    /// not [`SCHEMA_VERSION`], or [`LoadError::Invalid`] if it fails validation.
    ///
    /// ```
    /// use monomyth_core::World;
    /// use monomyth_core::doc_support::single_room_world;
    ///
    /// let json = serde_json::to_string(&single_room_world())?;
    /// let world = World::from_json_checked(&json)?;
    /// assert_eq!(world.meta.schema_version, monomyth_core::SCHEMA_VERSION);
    /// # Ok::<(), monomyth_core::LoadError>(())
    /// ```
    pub fn from_json_checked(json: &str) -> Result<World, LoadError> {
        let world: World = serde_json::from_str(json)?;
        if world.meta.schema_version != SCHEMA_VERSION {
            return Err(LoadError::SchemaMismatch {
                found: world.meta.schema_version,
                expected: SCHEMA_VERSION,
            });
        }
        world.validate()?;
        Ok(world)
    }
}
