//! Hybrid world generation: the deterministic procedural half.
//!
//! This crate assembles a [`monomyth_core::World`] from a `u64` seed by running an
//! ordered pipeline of [`ProceduralPass`]es. The output has real *structure* — a
//! connected location graph, the monomyth story arc, a cast of role-bearing
//! entities, and scattered items — but every text [`Content`](monomyth_core::Content)
//! slot is left [`Empty`](monomyth_core::Content::Empty). A separate, later content
//! phase (an LLM) fills those slots; it can never invent structure.
//!
//! # Determinism
//!
//! [`Generator::generate_structure`] is a pure function of its seed: the serialized
//! world is byte-identical across runs. The generator owns its own
//! [`rand_chacha::ChaCha8Rng`] for every procedural draw and never touches
//! [`World::rng`](monomyth_core::World::rng), which is the independent, quarantined
//! play-time stream that starts fresh at seed position zero.
//!
//! Passes are isolated into sub-streams: a root RNG seeded from the world seed
//! hands each pass, in pipeline order, a fresh child seed. Adding or reordering a
//! pass therefore does not perturb the draws of the others.
//!
//! # Example
//!
//! ```
//! use monomyth_gen::Generator;
//!
//! let generator = Generator::with_default_passes();
//! let a = serde_json::to_string(&generator.generate_structure(42)?)?;
//! let b = serde_json::to_string(&generator.generate_structure(42)?)?;
//! assert_eq!(a, b, "the same seed reproduces the same world");
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]

mod content;
mod content_passes;
mod error;
mod pass;
mod passes;

use monomyth_core::{
    Content, ContentKind, ContentPrompt, LocationId, Player, RngState, SCHEMA_VERSION, Story,
    World, WorldMeta, WorldState,
};
use monomyth_frameworks::MonomythStage;
use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub use content::{ContentContext, ContentPass, NamedProse, TextProse};
pub use content_passes::{EntityContentPass, LocationContentPass, TitleContentPass};
pub use error::GenError;
pub use pass::ProceduralPass;
pub use passes::{ArcPass, CastPass, ItemsPass, MAX_ROOMS, MIN_ROOMS, MapPass};

/// A generator with two ordered pipelines: the deterministic procedural passes
/// that assemble structure from a seed, and the content passes that later fill the
/// empty prose slots with grounded, LLM-authored text.
#[derive(Debug)]
pub struct Generator {
    passes: Vec<Box<dyn ProceduralPass>>,
    content_passes: Vec<Box<dyn ContentPass>>,
}

impl Generator {
    /// Build a generator from an explicit, ordered list of procedural passes and
    /// no content passes.
    ///
    /// The order is the determinism contract: each pass draws from its own
    /// sub-stream, but a pass may depend on structure an earlier pass produced
    /// (the cast and item passes require the map pass to have run first). Use
    /// [`with_pipelines`](Self::with_pipelines) to also install a content pipeline.
    #[must_use]
    pub fn new(passes: Vec<Box<dyn ProceduralPass>>) -> Self {
        Self {
            passes,
            content_passes: Vec::new(),
        }
    }

    /// Build a generator from explicit procedural and content pipelines.
    ///
    /// The two halves are independent: the procedural passes run in
    /// [`generate_structure`](Self::generate_structure), the content passes in
    /// [`fill_content`](Self::fill_content).
    #[must_use]
    pub fn with_pipelines(
        passes: Vec<Box<dyn ProceduralPass>>,
        content_passes: Vec<Box<dyn ContentPass>>,
    ) -> Self {
        Self {
            passes,
            content_passes,
        }
    }

    /// Build a generator with the standard procedural and content pipelines.
    ///
    /// The procedural order — [`MapPass`] → [`ArcPass`] → [`CastPass`] →
    /// [`ItemsPass`] — is fixed: the map lays down the location graph that the cast
    /// and items are placed into, and the arc grounds the story spine
    /// independently. The content order — [`TitleContentPass`] →
    /// [`LocationContentPass`] → [`EntityContentPass`] — fills the prose slots the
    /// procedural passes left empty.
    #[must_use]
    pub fn with_default_passes() -> Self {
        Self::with_pipelines(
            vec![
                Box::new(MapPass::new()),
                Box::new(ArcPass::new()),
                Box::new(CastPass::new()),
                Box::new(ItemsPass::new()),
            ],
            vec![
                Box::new(TitleContentPass::new()),
                Box::new(LocationContentPass::new()),
                Box::new(EntityContentPass::new()),
            ],
        )
    }

    /// Assemble a fully-structured world from `seed`, leaving all content empty.
    ///
    /// Builds a bootstrap world, seeds a root [`ChaCha8Rng`] from `seed`, and runs
    /// each pass in order with its own child sub-stream. The result is a pure
    /// function of `seed`.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::EmptyPipeline`] if the generator has no passes, or any
    /// error a pass raises when its structural preconditions are unmet.
    pub fn generate_structure(&self, seed: u64) -> Result<World, GenError> {
        if self.passes.is_empty() {
            return Err(GenError::EmptyPipeline);
        }

        let mut world = bootstrap_world(seed);

        // The root stream deterministically derives one child seed per pass, in
        // order, isolating each pass's draws from the others.
        let mut root = ChaCha8Rng::seed_from_u64(seed);
        for pass in &self.passes {
            let child_seed = root.next_u64();
            let mut pass_rng = ChaCha8Rng::seed_from_u64(child_seed);
            pass.apply(&mut world, &mut pass_rng)?;
        }

        Ok(world)
    }

    /// Fill `world`'s empty content slots by running the content passes in order.
    ///
    /// This is the non-deterministic content half of generation: each pass reads a
    /// slot's hint, retrieves ship-safe grounding, prompts the LLM, and fills the
    /// slot. It is quarantined from the procedural RNG — it never draws from
    /// [`World::rng`](monomyth_core::World::rng) — and changes only content, never
    /// structure (no elements, exits, roles, or keys are added, removed, or
    /// altered). Run it after [`generate_structure`](Self::generate_structure).
    ///
    /// # Errors
    ///
    /// Returns [`GenError::Llm`] if a generation call fails, or
    /// [`GenError::Knowledge`] if a grounding retrieval fails. A pass that fails
    /// partway leaves earlier fills in place.
    pub async fn fill_content(
        &self,
        world: &mut World,
        context: &ContentContext<'_>,
    ) -> Result<(), GenError> {
        for pass in &self.content_passes {
            pass.apply(world, context).await?;
        }
        Ok(())
    }
}

/// A structureless world the passes fill in: empty slotmaps, an empty arc, a
/// placeholder opening stage (overwritten by [`ArcPass`]), and a bootstrap player
/// at the null location (overwritten by [`MapPass`]).
fn bootstrap_world(seed: u64) -> World {
    World {
        meta: WorldMeta {
            seed,
            schema_version: SCHEMA_VERSION,
            title: Content::empty(ContentPrompt::new(ContentKind::Title, "the world's title")),
        },
        locations: slotmap_default(),
        entities: slotmap_default(),
        items: slotmap_default(),
        player: Player::new(LocationId::default()),
        story: Story {
            arc: Vec::new(),
            current_stage: MonomythStage::CallToAdventure,
            quests: slotmap_default(),
        },
        state: WorldState::default(),
        rng: RngState::new(seed),
    }
}

/// An empty typed slotmap, inferred from the field it initializes.
///
/// Wrapping [`Default::default`] keeps `bootstrap_world` readable and avoids naming
/// the `slotmap` crate directly (it is a transitive dependency through the model).
fn slotmap_default<T: Default>() -> T {
    T::default()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use monomyth_core::{Content, LocationId};
    use monomyth_frameworks::{MonomythStage, ProppRole};

    use super::{Generator, MAX_ROOMS, MIN_ROOMS};

    /// The canonical world used across the structural assertions.
    fn generated() -> monomyth_core::World {
        Generator::with_default_passes()
            .generate_structure(42)
            .expect("the default pipeline generates a world")
    }

    #[test]
    fn should_error_on_empty_pipeline() {
        let generator = Generator::new(Vec::new());
        assert!(matches!(
            generator.generate_structure(42),
            Err(super::GenError::EmptyPipeline),
        ));
    }

    #[test]
    fn room_count_is_within_bounds() {
        let count = generated().locations.len();
        assert!(
            (MIN_ROOMS..=MAX_ROOMS).contains(&count),
            "expected {MIN_ROOMS}..={MAX_ROOMS} rooms, got {count}",
        );
    }

    #[test]
    fn player_starts_at_a_real_location() {
        let world = generated();
        assert_ne!(world.player.location, LocationId::default());
        assert!(
            world.locations.get(world.player.location).is_some(),
            "player start must be a live location key",
        );
    }

    #[test]
    fn every_room_is_reachable_from_the_start() {
        let world = generated();
        let mut visited = BTreeSet::new();
        let mut frontier = vec![world.player.location];
        while let Some(current) = frontier.pop() {
            if !visited.insert(current) {
                continue;
            }
            for &neighbour in world.locations[current].exits.values() {
                if !visited.contains(&neighbour) {
                    frontier.push(neighbour);
                }
            }
        }
        assert_eq!(
            visited.len(),
            world.locations.len(),
            "the location graph must be fully connected",
        );
    }

    #[test]
    fn arc_covers_every_stage_in_order() {
        let world = generated();
        assert!(!world.story.arc.is_empty(), "the arc must be non-empty");
        let stages = world.story.arc.iter().map(|beat| beat.stage);
        assert!(
            stages.eq(MonomythStage::all().iter().copied()),
            "beats must be the monomyth stages, unique and in order",
        );
        assert_eq!(world.story.current_stage, MonomythStage::CallToAdventure);
    }

    #[test]
    fn cast_contains_a_hero_and_a_villain() {
        let world = generated();
        assert!(
            world
                .entities
                .values()
                .any(|entity| entity.role == Some(ProppRole::Hero)),
            "the cast must include a Hero",
        );
        assert!(
            world
                .entities
                .values()
                .any(|entity| entity.role == Some(ProppRole::Villain)),
            "the cast must include a Villain",
        );
    }

    #[test]
    fn entity_location_relation_is_in_sync() {
        let world = generated();
        // Every placed entity appears in its room's entity set.
        for (entity_id, entity) in &world.entities {
            let location = entity.location.expect("cast entities are placed");
            assert!(
                world.locations[location].entities.contains(&entity_id),
                "location must list the entity placed there",
            );
        }
        // Every entity a room lists points back at that room.
        for (location_id, location) in &world.locations {
            for entity_id in &location.entities {
                assert_eq!(
                    world.entities[*entity_id].location,
                    Some(location_id),
                    "entity must point back at the room listing it",
                );
            }
        }
    }

    #[test]
    fn every_content_slot_is_empty() {
        let world = generated();
        let mut slots: Vec<&Content> = vec![&world.meta.title];
        for location in world.locations.values() {
            slots.push(&location.name);
            slots.push(&location.description);
        }
        for entity in world.entities.values() {
            slots.push(&entity.name);
            slots.push(&entity.description);
        }
        for item in world.items.values() {
            slots.push(&item.name);
            slots.push(&item.description);
        }
        for beat in &world.story.arc {
            slots.push(&beat.synopsis);
        }
        for quest in world.story.quests.values() {
            slots.push(&quest.title);
        }
        assert!(
            slots.iter().all(|slot| !slot.is_filled()),
            "all content slots must remain empty in the procedural phase",
        );
    }
}
