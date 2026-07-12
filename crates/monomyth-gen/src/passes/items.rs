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

/// Tunable knobs for item placement.
///
/// `items_min..=items_max` items are scattered across the world, drawn once
/// from the pass's sub-stream. The invariant `min <= max` must hold;
/// [`draw_range_inclusive`] debug-asserts it.
#[derive(Debug, Clone, Copy)]
pub struct ItemsConfig {
    /// The fewest items scattered across the world (must be `<= items_max`).
    pub items_min: usize,
    /// The most items scattered across the world (must be `>= items_min`).
    pub items_max: usize,
}

impl Default for ItemsConfig {
    fn default() -> Self {
        Self {
            items_min: MIN_ITEMS,
            items_max: MAX_ITEMS,
        }
    }
}

/// Places items into rooms across the location graph.
#[derive(Debug, Default, Clone, Copy)]
pub struct ItemsPass {
    config: ItemsConfig,
}

impl ItemsPass {
    /// Construct the items pass with the default [`ItemsConfig`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct the items pass with an explicit configuration.
    #[must_use]
    pub fn with_config(config: ItemsConfig) -> Self {
        Self { config }
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

        let item_count = draw_range_inclusive(rng, self.config.items_min, self.config.items_max);
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
