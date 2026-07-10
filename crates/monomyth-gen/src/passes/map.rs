//! [`MapPass`]: build a small, connected location graph.
//!
//! The pass inserts a seed-chosen number of rooms, wires them into a random
//! spanning tree (so every room is reachable — no orphans), then picks the
//! player's start room. All name and description slots are left
//! [`Empty`](monomyth_core::Content::Empty) with hints locating each room in the
//! graph for the later content phase.

use std::collections::{BTreeMap, BTreeSet};

use monomyth_core::{Content, ContentKind, ContentPrompt, Direction, Location, LocationId, World};
use rand_chacha::ChaCha8Rng;

use crate::error::GenError;
use crate::pass::{ProceduralPass, draw_range_inclusive};

/// The fewest rooms a generated world may contain.
pub const MIN_ROOMS: usize = 5;
/// The most rooms a generated world may contain.
pub const MAX_ROOMS: usize = 9;

/// The order in which candidate exit directions are tried when wiring an edge.
///
/// Fixed so wiring is deterministic; the structural variety comes from the
/// seed-driven choice of which room is each new room's parent.
const DIRECTION_ORDER: [Direction; 6] = [
    Direction::North,
    Direction::East,
    Direction::South,
    Direction::West,
    Direction::Up,
    Direction::Down,
];

/// Builds the connected location graph and sets the player's start room.
#[derive(Debug, Default, Clone, Copy)]
pub struct MapPass;

impl MapPass {
    /// Construct the map pass.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

/// The first direction not already used by `exits`, or `None` if all six are taken.
fn free_direction(exits: &BTreeMap<Direction, LocationId>) -> Option<Direction> {
    DIRECTION_ORDER
        .iter()
        .copied()
        .find(|direction| !exits.contains_key(direction))
}

/// Build an empty content slot describing room `index` of `count`.
fn room_slot(kind: ContentKind, role: &str, index: usize, count: usize) -> Content {
    let hint = format!(
        "{role} of location {} of {count} in the world graph",
        index + 1
    );
    Content::empty(ContentPrompt::new(kind, hint))
}

impl ProceduralPass for MapPass {
    fn name(&self) -> &'static str {
        "MapPass"
    }

    fn apply(&self, world: &mut World, rng: &mut ChaCha8Rng) -> Result<(), GenError> {
        let count = draw_range_inclusive(rng, MIN_ROOMS, MAX_ROOMS);

        let mut ids = Vec::with_capacity(count);
        for index in 0..count {
            let location = Location {
                name: room_slot(ContentKind::Name, "name", index, count),
                description: room_slot(ContentKind::Description, "interior", index, count),
                exits: BTreeMap::new(),
                entities: BTreeSet::new(),
                items: BTreeSet::new(),
            };
            ids.push(world.locations.insert(location));
        }

        for child in 1..count {
            let mut parent = draw_range_inclusive(rng, 0, child - 1);
            if free_direction(&world.locations[ids[parent]].exits).is_none() {
                parent = (0..child)
                    .find(|&candidate| {
                        free_direction(&world.locations[ids[candidate]].exits).is_some()
                    })
                    .ok_or(GenError::Invariant(
                        "no earlier room has a free exit while wiring the map",
                    ))?;
            }

            let parent_id = ids[parent];
            let child_id = ids[child];
            let direction = free_direction(&world.locations[parent_id].exits)
                .ok_or(GenError::Invariant("selected parent lost its free exit"))?;

            world.locations[parent_id].exits.insert(direction, child_id);
            world.locations[child_id]
                .exits
                .insert(direction.opposite(), parent_id);
        }

        let start = draw_range_inclusive(rng, 0, count - 1);
        world.player.location = ids[start];

        Ok(())
    }
}
