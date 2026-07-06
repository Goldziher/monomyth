//! Inhabitants and objects of the world: [`Entity`], [`Item`], and the [`Player`].
//!
//! Every content-bearing field here is a [`Content`] slot so prose regenerates
//! independently of structure. Character-model facets ([`ProppRole`],
//! [`Archetype`]) come from `monomyth-frameworks`; they are structural priors the
//! generator sets, not free-form strings.

use std::collections::BTreeSet;

use monomyth_frameworks::{Archetype, ProppRole};
use serde::{Deserialize, Serialize};

use crate::content::Content;
use crate::ids::{ItemId, LocationId};

/// The default maximum health a freshly created [`Player`] starts with.
///
/// A named constant rather than a literal so the engine and any generator agree
/// on the starting value without a magic number.
pub const DEFAULT_MAX_HEALTH: u32 = 100;

/// What sort of thing an [`Entity`] is, driving how the engine and frontends
/// treat it.
///
/// An enum rather than a set of booleans: the categories are mutually exclusive
/// and closed, so a single tag is clearer than several flags.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum EntityKind {
    /// A speaking, reasoning character.
    Npc,
    /// A non-speaking living creature.
    Creature,
    /// A divine or mythic being.
    Deity,
    /// An inert but notable fixture (a statue, an altar) that is not an [`Item`].
    Object,
}

/// A dramatis persona or fixture occupying (at most) one location.
///
/// `role` and `archetype` are optional because not every entity carries a story
/// function; a background creature may have neither.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entity {
    /// The entity's name slot.
    pub name: Content,
    /// The entity's description slot.
    pub description: Content,
    /// What kind of entity this is.
    pub kind: EntityKind,
    /// The Propp dramatis-personae role this entity plays, if any.
    pub role: Option<ProppRole>,
    /// The story archetype this entity wears, if any.
    pub archetype: Option<Archetype>,
    /// Where the entity currently is, or `None` if unplaced.
    ///
    /// This is one side of the entity/location relationship whose other side is
    /// [`Location::entities`](crate::Location::entities); the two must be kept in
    /// sync by the caller. The engine does not move entities yet, so nothing
    /// maintains this invariant automatically.
    pub location: Option<LocationId>,
}

/// A takeable or fixed object that can appear in a location or an inventory.
///
/// `portable` is a data field describing the object, not a control-flow flag; the
/// engine reads it to decide whether a `Take` succeeds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// The item's name slot.
    pub name: Content,
    /// The item's description slot.
    pub description: Content,
    /// Whether the item can be picked up and carried.
    pub portable: bool,
}

/// The player avatar: a position, a carried inventory, and health.
///
/// The inventory is a [`BTreeSet`] of [`ItemId`] for deterministic iteration and
/// serialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Player {
    /// The location the player currently occupies.
    pub location: LocationId,
    /// Item ids the player is carrying.
    pub inventory: BTreeSet<ItemId>,
    /// Current health.
    pub health: u32,
    /// The cap gameplay should clamp [`health`](Self::health) to; healing must not
    /// raise `health` above this value.
    pub max_health: u32,
}

impl Player {
    /// A player standing at `location`, empty-handed and at full health.
    ///
    /// ```
    /// use monomyth_core::{LocationId, Player, DEFAULT_MAX_HEALTH};
    ///
    /// let player = Player::new(LocationId::default());
    /// assert_eq!(player.health, DEFAULT_MAX_HEALTH);
    /// assert_eq!(player.max_health, DEFAULT_MAX_HEALTH);
    /// assert!(player.inventory.is_empty());
    /// ```
    #[must_use]
    pub fn new(location: LocationId) -> Self {
        Self {
            location,
            inventory: BTreeSet::new(),
            health: DEFAULT_MAX_HEALTH,
            max_health: DEFAULT_MAX_HEALTH,
        }
    }
}
