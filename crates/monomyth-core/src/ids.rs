//! Typed, generational identifiers for everything referenced across the model.
//!
//! Cross-references in the world graph (a location's exits, a player's
//! inventory, an entity's home) must survive insertion, removal, and
//! serialization without silently rebinding to the wrong element. A raw `Vec`
//! index cannot: deleting an earlier element shifts every later index. `slotmap`
//! generational keys solve this — a key encodes both a slot and a generation, so
//! a stale key from a removed element is detected rather than aliased. These
//! newtypes give each collection its own key space, so an [`ItemId`] can never be
//! used where a [`LocationId`] is expected.

use slotmap::new_key_type;

new_key_type! {
    /// Stable key for a [`Location`](crate::Location) in [`World::locations`](crate::World).
    pub struct LocationId;

    /// Stable key for an [`Entity`](crate::Entity) in [`World::entities`](crate::World).
    pub struct EntityId;

    /// Stable key for an [`Item`](crate::Item) in [`World::items`](crate::World).
    pub struct ItemId;

    /// Stable key for a [`Quest`](crate::Quest) in [`Story::quests`](crate::Story).
    pub struct QuestId;

    /// Stable key for a [`NarrativeNode`](crate::NarrativeNode) in
    /// [`NarrativeStructure::nodes`](crate::NarrativeStructure).
    pub struct NarrativeNodeId;
}
