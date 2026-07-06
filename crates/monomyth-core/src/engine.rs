//! The pure, deterministic state machine: [`apply`] turns an [`Action`] into a
//! list of [`Event`]s (or a specific [`ActionError`]).
//!
//! [`apply`] is a pure function of `(world, action)`: it reads no clock, spawns no
//! thread RNG, and performs no IO. Any randomness comes from the world's
//! [`RngState`](crate::RngState) and is persisted back, so a session replays
//! exactly from its seed and action log. A successful action advances
//! [`WorldState::turn`](crate::WorldState); a failing one leaves the world
//! unchanged so the caller can retry.

use serde::{Deserialize, Serialize};

use crate::ids::{EntityId, ItemId, LocationId};
use crate::world::{Direction, World};

/// A command the player issues, to be interpreted by [`apply`].
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Action {
    /// Move the player through the exit in `direction`.
    Move(Direction),
    /// Pick up the item and place it in the player's inventory.
    Take(ItemId),
    /// Drop a carried item into the current location.
    Drop(ItemId),
    /// Inspect a target and report its description.
    Examine(ExamineTarget),
    /// Pass the turn without acting.
    Wait,
}

/// What an [`Action::Examine`] inspects.
///
/// Examining an [`Item`](ExamineTarget::Item) or [`Entity`](ExamineTarget::Entity)
/// succeeds for any id known to the world, regardless of the player's proximity to
/// it. This is intentional for now; proximity checks belong to future `Attack` and
/// `Talk` actions that actually require the target to be reachable.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ExamineTarget {
    /// The player's current location.
    Location,
    /// A specific item (in the room or inventory).
    Item(ItemId),
    /// A specific entity.
    Entity(EntityId),
}

/// A fact about what changed, returned in order from [`apply`].
///
/// Events are the observable record of a turn; a frontend renders them and a
/// replay can be validated against them.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Event {
    /// The player moved between two locations.
    Moved {
        /// The location departed.
        from: LocationId,
        /// The location entered.
        to: LocationId,
    },
    /// The player picked up an item.
    Took(ItemId),
    /// The player dropped an item.
    Dropped(ItemId),
    /// A target was examined, yielding descriptive text.
    Examined {
        /// The description reported to the player.
        text: String,
    },
    /// The player waited, passing the turn.
    Waited,
}

/// Why an [`Action`] could not be applied.
///
/// Every variant names the specific offending id or direction so a caller can
/// report or recover precisely; the engine never panics on a bad reference.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, thiserror::Error)]
pub enum ActionError {
    /// The current location has no exit in the requested direction.
    #[error("no exit to the {0:?}")]
    NoExit(Direction),
    /// The item is not present in the current location.
    #[error("item {0:?} is not here")]
    ItemNotHere(ItemId),
    /// The item is not in the player's inventory.
    #[error("item {0:?} is not being carried")]
    ItemNotInInventory(ItemId),
    /// The item cannot be picked up.
    #[error("item {0:?} cannot be taken")]
    NotPortable(ItemId),
    /// No item exists for the given id.
    #[error("unknown item {0:?}")]
    UnknownItem(ItemId),
    /// No entity exists for the given id.
    #[error("unknown entity {0:?}")]
    UnknownEntity(EntityId),
    /// No location exists for the given id (a corrupt world reference).
    #[error("unknown location {0:?}")]
    UnknownLocation(LocationId),
}

/// Apply `action` to `world`, returning the events it produced.
///
/// On success the world is mutated in place and its turn counter advances; on
/// error the world is left unchanged. Pure and deterministic: the same
/// `(world, action)` always yields the same result and the same post-state.
///
/// # Errors
///
/// Returns the [`ActionError`] naming the specific reason the action could not be
/// applied (no such exit, item not here, not portable, and so on).
///
/// ```
/// use monomyth_core::{apply, Action, Direction, Event};
/// # use monomyth_core::doc_support::single_room_world;
/// let mut world = single_room_world();
/// // A room with no exits: moving north fails with a precise error.
/// assert_eq!(apply(&mut world, Action::Move(Direction::North)).unwrap_err(),
///            monomyth_core::ActionError::NoExit(Direction::North));
/// // Waiting always succeeds and advances the turn.
/// assert_eq!(apply(&mut world, Action::Wait)?, vec![Event::Waited]);
/// assert_eq!(world.state.turn, 1);
/// # Ok::<(), monomyth_core::ActionError>(())
/// ```
pub fn apply(world: &mut World, action: Action) -> Result<Vec<Event>, ActionError> {
    let events = match action {
        Action::Move(direction) => move_player(world, direction)?,
        Action::Take(item) => take_item(world, item)?,
        Action::Drop(item) => drop_item(world, item)?,
        Action::Examine(target) => examine(world, target)?,
        Action::Wait => vec![Event::Waited],
    };
    // Only a successful action consumes a turn, so an error is retryable.
    world.state.turn += 1;
    Ok(events)
}

/// Move the player through an exit, validating both endpoints.
fn move_player(world: &mut World, direction: Direction) -> Result<Vec<Event>, ActionError> {
    let from = world.player.location;
    let origin = world
        .locations
        .get(from)
        .ok_or(ActionError::UnknownLocation(from))?;
    let to = *origin
        .exits
        .get(&direction)
        .ok_or(ActionError::NoExit(direction))?;
    if !world.locations.contains_key(to) {
        return Err(ActionError::UnknownLocation(to));
    }
    world.player.location = to;
    Ok(vec![Event::Moved { from, to }])
}

/// Take a portable item present in the current location into the inventory.
fn take_item(world: &mut World, item_id: ItemId) -> Result<Vec<Event>, ActionError> {
    let portable = world
        .items
        .get(item_id)
        .ok_or(ActionError::UnknownItem(item_id))?
        .portable;
    let location_id = world.player.location;
    let location = world
        .locations
        .get_mut(location_id)
        .ok_or(ActionError::UnknownLocation(location_id))?;
    if !location.items.contains(&item_id) {
        return Err(ActionError::ItemNotHere(item_id));
    }
    if !portable {
        return Err(ActionError::NotPortable(item_id));
    }
    location.items.remove(&item_id);
    world.player.inventory.insert(item_id);
    Ok(vec![Event::Took(item_id)])
}

/// Drop a carried item into the current location.
fn drop_item(world: &mut World, item_id: ItemId) -> Result<Vec<Event>, ActionError> {
    if !world.player.inventory.contains(&item_id) {
        return Err(ActionError::ItemNotInInventory(item_id));
    }
    let location_id = world.player.location;
    let location = world
        .locations
        .get_mut(location_id)
        .ok_or(ActionError::UnknownLocation(location_id))?;
    location.items.insert(item_id);
    world.player.inventory.remove(&item_id);
    Ok(vec![Event::Dropped(item_id)])
}

/// Report a target's description as [`Event::Examined`].
fn examine(world: &World, target: ExamineTarget) -> Result<Vec<Event>, ActionError> {
    let text = match target {
        ExamineTarget::Location => {
            let location_id = world.player.location;
            let location = world
                .locations
                .get(location_id)
                .ok_or(ActionError::UnknownLocation(location_id))?;
            described(&location.description)
        }
        ExamineTarget::Item(item_id) => {
            let item = world
                .items
                .get(item_id)
                .ok_or(ActionError::UnknownItem(item_id))?;
            described(&item.description)
        }
        ExamineTarget::Entity(entity_id) => {
            let entity = world
                .entities
                .get(entity_id)
                .ok_or(ActionError::UnknownEntity(entity_id))?;
            described(&entity.description)
        }
    };
    Ok(vec![Event::Examined { text }])
}

/// The filled description, or the prompt hint while the slot is still empty.
///
/// The engine never fabricates prose; an unfilled slot surfaces its authoring
/// hint so a frontend can show a deterministic placeholder before the content
/// pass runs.
fn described(description: &crate::content::Content) -> String {
    description
        .value()
        .cloned()
        .unwrap_or_else(|| description.prompt().hint.clone())
}
