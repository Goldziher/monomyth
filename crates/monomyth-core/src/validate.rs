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

use crate::ids::{EntityId, ItemId, LocationId, NarrativeNodeId, QuestId};
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
    /// A node's [`stage`](crate::NarrativeNode::stage) has its primary label
    /// duplicated in its own alternatives (see
    /// [`ScoredOne::primary_duplicated_in_alternatives`](crate::ScoredOne::primary_duplicated_in_alternatives)).
    #[error("node {0:?} has its stage primary duplicated in its alternatives")]
    NodeStagePrimaryDuplicated(NarrativeNodeId),
    /// A node's [`situation`](crate::NarrativeNode::situation) has its primary
    /// label duplicated in its own alternatives.
    #[error("node {0:?} has its situation primary duplicated in its alternatives")]
    NodeSituationPrimaryDuplicated(NarrativeNodeId),
    /// An entity's [`role`](crate::Entity::role) has its primary label duplicated
    /// in its own alternatives.
    #[error("entity {0:?} has its role primary duplicated in its alternatives")]
    EntityRolePrimaryDuplicated(EntityId),
    /// An entity's [`archetype`](crate::Entity::archetype) has its primary label
    /// duplicated in its own alternatives.
    #[error("entity {0:?} has its archetype primary duplicated in its alternatives")]
    EntityArchetypePrimaryDuplicated(EntityId),
    /// [`Story::plot`](crate::Story::plot) has its primary label duplicated in its
    /// own alternatives.
    #[error("the story plot has its primary duplicated in its alternatives")]
    StoryPlotPrimaryDuplicated,
    /// A quest's [`situation`](crate::Quest::situation) has its primary label
    /// duplicated in its own alternatives.
    #[error("quest {0:?} has its situation primary duplicated in its alternatives")]
    QuestSituationPrimaryDuplicated(QuestId),
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

        self.check_scored_invariants()?;

        Ok(())
    }

    /// Check every [`ScoredOne`](crate::ScoredOne)-typed field's
    /// primary-not-in-alternatives invariant.
    ///
    /// A derived [`Deserialize`](serde::Deserialize) does not route through
    /// [`ScoredOne::insert_alternative`](crate::ScoredOne::insert_alternative), so
    /// a hand-authored or externally produced world can smuggle in a duplicated
    /// key; this is the load-time gate that catches it. Node, entity, and quest
    /// keys are iterated in sorted order, matching every other check in this
    /// module, so the smallest offending id is reported first.
    fn check_scored_invariants(&self) -> Result<(), WorldError> {
        self.check_node_scored_invariants()?;
        self.check_entity_scored_invariants()?;
        self.check_story_scored_invariants()?;
        Ok(())
    }

    /// Check the `stage` and `situation` invariant on every narrative node.
    fn check_node_scored_invariants(&self) -> Result<(), WorldError> {
        let mut keys: Vec<NarrativeNodeId> = self.story.structure.nodes.keys().collect();
        keys.sort_unstable();
        for node_id in keys {
            let node = &self.story.structure.nodes[node_id];
            if node.stage.primary_duplicated_in_alternatives() {
                return Err(WorldError::NodeStagePrimaryDuplicated(node_id));
            }
            let situation_duplicated = node
                .situation
                .as_ref()
                .is_some_and(crate::scored::ScoredOne::primary_duplicated_in_alternatives);
            if situation_duplicated {
                return Err(WorldError::NodeSituationPrimaryDuplicated(node_id));
            }
        }
        Ok(())
    }

    /// Check the `role` and `archetype` invariant on every entity.
    fn check_entity_scored_invariants(&self) -> Result<(), WorldError> {
        for entity_id in Self::sorted_keys(&self.entities) {
            let entity = &self.entities[entity_id];
            let role_duplicated = entity
                .role
                .as_ref()
                .is_some_and(crate::scored::ScoredOne::primary_duplicated_in_alternatives);
            if role_duplicated {
                return Err(WorldError::EntityRolePrimaryDuplicated(entity_id));
            }
            let archetype_duplicated = entity
                .archetype
                .as_ref()
                .is_some_and(crate::scored::ScoredOne::primary_duplicated_in_alternatives);
            if archetype_duplicated {
                return Err(WorldError::EntityArchetypePrimaryDuplicated(entity_id));
            }
        }
        Ok(())
    }

    /// Check the story's `plot` invariant and every quest's `situation` invariant.
    fn check_story_scored_invariants(&self) -> Result<(), WorldError> {
        let plot_duplicated = self
            .story
            .plot
            .as_ref()
            .is_some_and(crate::scored::ScoredOne::primary_duplicated_in_alternatives);
        if plot_duplicated {
            return Err(WorldError::StoryPlotPrimaryDuplicated);
        }
        for quest_id in Self::sorted_keys(&self.story.quests) {
            let quest_duplicated = self.story.quests[quest_id]
                .situation
                .as_ref()
                .is_some_and(crate::scored::ScoredOne::primary_duplicated_in_alternatives);
            if quest_duplicated {
                return Err(WorldError::QuestSituationPrimaryDuplicated(quest_id));
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

#[cfg(test)]
mod tests {
    use monomyth_frameworks::ProppRole;

    use super::*;
    use crate::doc_support::single_room_world;
    use crate::entity::{Entity, EntityKind};
    use crate::scored::ScoredOne;
    use crate::weight::Weight;
    use crate::{Content, ContentKind, ContentPrompt};

    /// A derived `Deserialize` does not route through
    /// [`ScoredOne::insert_alternative`], so a hand-edited world with the
    /// mandatory `NarrativeNode.stage` primary duplicated into its own
    /// alternatives must still be rejected — proving the construction-time
    /// invariant has a real deserialization-time gap that `World::validate`
    /// closes.
    #[test]
    fn should_reject_a_node_stage_with_primary_duplicated_in_alternatives() {
        let world = single_room_world();
        let mut json: serde_json::Value =
            serde_json::to_value(&world).expect("world serializes to a JSON value");

        // `SlotMap` serializes as a plain array of `{value, version}` slots
        // indexed by slot position, not a keyed object; the fixture's sole node
        // is the only live (non-null-`value`) slot.
        let nodes = json["story"]["structure"]["nodes"]
            .as_array_mut()
            .expect("nodes serializes to an array");
        let slot = nodes
            .iter_mut()
            .find(|slot| !slot["value"].is_null())
            .expect("the fixture has exactly one live node");
        let node = &mut slot["value"];
        let primary = node["stage"]["primary"].clone();
        node["stage"]["alternatives"] = serde_json::json!({});
        node["stage"]["alternatives"][primary.as_str().expect("primary is a string label")] =
            serde_json::json!(500);

        let root = world.story.structure.root();
        let corrupted = serde_json::to_string(&json).expect("value serializes back to a string");
        let error = World::from_json_checked(&corrupted).expect_err(
            "a node stage with its primary duplicated in alternatives must be rejected",
        );
        assert!(
            matches!(
                error,
                LoadError::Invalid(WorldError::NodeStagePrimaryDuplicated(id)) if id == root
            ),
            "expected NodeStagePrimaryDuplicated({root:?}), got {error:?}",
        );
    }

    /// The same gap, exercised on an `Option<ScoredOne<_>>` field
    /// (`Entity.role`) rather than the mandatory `NarrativeNode.stage`, via
    /// `World::validate` directly rather than through JSON text (the invalid
    /// value is built by bypassing `insert_alternative`, the same way a derived
    /// `Deserialize` would).
    #[test]
    fn should_reject_an_entity_role_with_primary_duplicated_in_alternatives() {
        let mut world = single_room_world();
        let room = world.player.location;

        let role = ScoredOne::new(ProppRole::Hero);
        // insert_alternative refuses a key equal to primary, so the invariant
        // violation is forced through a JSON round trip, mirroring what a
        // derived Deserialize over hand-authored JSON could smuggle in.
        let mut role_json = serde_json::to_value(&role).expect("role serializes");
        let primary = role_json["primary"].clone();
        role_json["alternatives"][primary.as_str().expect("primary is a string label")] =
            serde_json::json!(Weight::new(300).permille());
        let broken_role: ScoredOne<ProppRole> =
            serde_json::from_value(role_json).expect("role deserializes");

        let entity = Entity {
            name: Content::empty(ContentPrompt::new(ContentKind::Name, "a hero")),
            description: Content::empty(ContentPrompt::new(ContentKind::Description, "")),
            kind: EntityKind::Npc,
            role: Some(broken_role),
            archetype: None,
            location: Some(room),
        };
        let entity_id = world.entities.insert(entity);
        world.locations[room].entities.insert(entity_id);

        let error = world.validate().expect_err(
            "an entity role with its primary duplicated in alternatives must be rejected",
        );
        assert_eq!(error, WorldError::EntityRolePrimaryDuplicated(entity_id));
    }
}
