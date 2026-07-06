//! [`ItemsPass`]: scatter a handful of items across the location graph.
//!
//! Each item is placed into a seed-chosen room and its id added to that room's
//! item set. Some items are portable and some are fixtures. Name and description
//! slots are left empty for the content phase.

use monomyth_core::{Content, ContentKind, ContentPrompt, Item, World};
use rand_chacha::ChaCha8Rng;

use crate::error::GenError;
use crate::pass::{ProceduralPass, draw_bool, draw_range_inclusive};

/// The fewest items scattered across the world.
const MIN_ITEMS: usize = 2;
/// The most items scattered across the world.
const MAX_ITEMS: usize = 6;

/// Places items into rooms across the location graph.
#[derive(Debug, Default, Clone, Copy)]
pub struct ItemsPass;

impl ItemsPass {
    /// Construct the items pass.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ProceduralPass for ItemsPass {
    fn name(&self) -> &'static str {
        "ItemsPass"
    }

    fn apply(&self, world: &mut World, rng: &mut ChaCha8Rng) -> Result<(), GenError> {
        let rooms: Vec<_> = world.locations.keys().collect();
        if rooms.is_empty() {
            return Err(GenError::NoLocations { pass: self.name() });
        }

        let item_count = draw_range_inclusive(rng, MIN_ITEMS, MAX_ITEMS);
        for index in 0..item_count {
            let portable = draw_bool(rng);
            let room = rooms[draw_range_inclusive(rng, 0, rooms.len() - 1)];

            let kind = if portable {
                "portable item"
            } else {
                "fixed item"
            };
            let name_hint = format!("name of {kind} {}", index + 1);
            let description_hint = format!("description of {kind} {}", index + 1);

            let item = Item {
                name: Content::empty(ContentPrompt::new(ContentKind::Name, name_hint)),
                description: Content::empty(ContentPrompt::new(
                    ContentKind::Description,
                    description_hint,
                )),
                portable,
            };

            let item_id = world.items.insert(item);
            world.locations[room].items.insert(item_id);
        }

        Ok(())
    }
}
