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
//! hands each pass, in pipeline order, a fresh child seed. Because the child
//! seeds are derived sequentially, *appending* a pass leaves every earlier pass's
//! sub-stream seed unchanged; inserting or reordering a pass shifts the seeds of
//! everything downstream. Within a pass, changing how many draws it makes never
//! affects any other pass's sub-stream.
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

mod config;
mod content;
mod content_passes;
mod error;
mod pass;
mod passes;

use monomyth_core::{
    Content, ContentKind, ContentPrompt, LocationId, NarrativeStructure, Player, RngState,
    SCHEMA_VERSION, Story, World, WorldMeta, WorldState,
};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

pub use config::GenerationConfig;
pub use content::{ContentConfig, ContentContext, ContentPass, NamedProse, TextProse};
pub use content_passes::{
    EntityContentPass, ItemContentPass, LocationContentPass, TitleContentPass,
};
pub use error::GenError;
pub use pass::ProceduralPass;
pub use passes::{
    BackbonePass, BeatConfig, BeatPass, CastConfig, CastPass, ItemsConfig, ItemsPass, MAX_ROOMS,
    MIN_ROOMS, MapConfig, MapPass, NarrativeConfig, PlotChoice,
};

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
    /// The procedural order — [`BackbonePass`] → [`BeatPass`] → [`MapPass`] →
    /// [`CastPass`] → [`ItemsPass`] — is fixed and plan-first: the backbone grounds
    /// the branching narrative structure independently, the beat pass grows it to a
    /// playable length, then the map lays down the location graph that the cast and
    /// items are placed into. The content order —
    /// [`TitleContentPass`] → [`LocationContentPass`] → [`EntityContentPass`] →
    /// [`ItemContentPass`] — fills the prose slots the procedural passes left empty.
    #[must_use]
    pub fn with_default_passes() -> Self {
        Self::with_config(&GenerationConfig::default())
    }

    /// Build a generator with the standard pipelines, parameterizing the passes
    /// that read configuration from `config`.
    ///
    /// Identical to [`with_default_passes`](Self::with_default_passes) except the
    /// [`BackbonePass`] is built from `config` (its fork probability) rather than
    /// its own default. With `config == &GenerationConfig::default()` the output is
    /// byte-identical to `with_default_passes`, so an unconfigured run never drifts
    /// the determinism golden — this equivalence is pinned by a test in
    /// `tests/determinism.rs`.
    #[must_use]
    pub fn with_config(config: &GenerationConfig) -> Self {
        Self::with_pipelines(
            vec![
                Box::new(BackbonePass::with_config(NarrativeConfig {
                    plot: PlotChoice::Seeded,
                    fork_chance_permille: config.fork_chance_permille,
                })),
                Box::new(BeatPass::with_config(BeatConfig {
                    beats_per_stage_min: config.beats_per_stage_min,
                    beats_per_stage_max: config.beats_per_stage_max,
                })),
                Box::new(MapPass::with_config(MapConfig {
                    rooms_min: config.rooms_min,
                    rooms_max: config.rooms_max,
                })),
                Box::new(CastPass::with_config(CastConfig {
                    max_extra_cast: config.max_extra_cast,
                })),
                Box::new(ItemsPass::with_config(ItemsConfig {
                    items_min: config.items_min,
                    items_max: config.items_max,
                })),
            ],
            vec![
                Box::new(TitleContentPass::new()),
                Box::new(LocationContentPass::new()),
                Box::new(EntityContentPass::new()),
                Box::new(ItemContentPass::new()),
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
    /// Returns [`GenError::ContentPass`], naming the failing pass, if a pass
    /// fails — wrapping a [`GenError::Llm`] if a generation call fails, or a
    /// [`GenError::Knowledge`] if a grounding retrieval fails. A pass that fails
    /// partway leaves earlier fills in place.
    pub async fn fill_content(
        &self,
        world: &mut World,
        context: &ContentContext<'_>,
    ) -> Result<(), GenError> {
        for pass in &self.content_passes {
            pass.apply(world, context)
                .await
                .map_err(|source| GenError::ContentPass {
                    pass: pass.name(),
                    source: Box::new(source),
                })?;
        }
        Ok(())
    }
}

/// A structureless world the passes fill in: empty slotmaps, an empty narrative
/// structure (overwritten by [`BackbonePass`]), and a bootstrap player at the null
/// location (overwritten by [`MapPass`]).
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
            structure: NarrativeStructure::default(),
            plot: None,
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
    use monomyth_frameworks::{MonomythStage, ProppRole, arc_functions};

    use super::{BackbonePass, Generator, MAX_ROOMS, MIN_ROOMS, NarrativeConfig, PlotChoice};

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
    fn generated_world_is_internally_valid() {
        for seed in [1u64, 7, 42, 99, 2024] {
            let world = Generator::with_default_passes()
                .generate_structure(seed)
                .expect("generation succeeds");
            assert_eq!(
                world.validate(),
                Ok(()),
                "seed {seed} must produce an internally consistent world",
            );
        }
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
    fn spine_covers_all_mandatory_stages_in_order() {
        let world = generated();
        let structure = &world.story.structure;
        assert_eq!(structure.validate(), Ok(()), "the structure must be valid");

        let spine_stages: Vec<MonomythStage> = structure
            .spine()
            .iter()
            .map(|&id| {
                *structure
                    .node(id)
                    .expect("spine node exists")
                    .stage
                    .primary()
            })
            .collect();

        let canonical = MonomythStage::all();
        let positions: Vec<usize> = spine_stages
            .iter()
            .map(|stage| {
                canonical
                    .iter()
                    .position(|candidate| candidate == stage)
                    .expect("spine stage is canonical")
            })
            .collect();
        assert!(
            positions.windows(2).all(|pair| pair[0] <= pair[1]),
            "spine stages must be in weakly increasing id order, got {positions:?}",
        );

        for stage in canonical {
            if !stage.info().optional {
                assert!(
                    spine_stages.contains(stage),
                    "mandatory stage {stage:?} must be on the spine",
                );
            }
        }

        let root = structure.root();
        assert_eq!(world.state.cursor, root);
        assert_eq!(
            *structure.node(root).expect("root exists").stage.primary(),
            MonomythStage::CallToAdventure,
        );
    }

    #[test]
    fn all_forks_skip_every_optional_stage_and_reconverge() {
        let generator =
            Generator::new(vec![Box::new(BackbonePass::with_config(NarrativeConfig {
                plot: PlotChoice::Seeded,
                fork_chance_permille: 1000,
            }))]);
        let world = generator
            .generate_structure(42)
            .expect("backbone-only generation succeeds");
        let structure = &world.story.structure;
        assert_eq!(structure.validate(), Ok(()));

        let spine_stages: Vec<MonomythStage> = structure
            .spine()
            .iter()
            .map(|&id| *structure.node(id).expect("spine node").stage.primary())
            .collect();
        let mandatory: Vec<MonomythStage> = MonomythStage::all()
            .iter()
            .copied()
            .filter(|stage| !stage.info().optional)
            .collect();
        assert_eq!(
            spine_stages, mandatory,
            "an all-forks spine must be exactly the mandatory stages",
        );

        assert_eq!(
            structure.nodes.len(),
            MonomythStage::all().len(),
            "forking must not add or remove nodes",
        );

        let on_spine: BTreeSet<MonomythStage> = spine_stages.iter().copied().collect();
        let optional_off_spine = structure
            .nodes
            .values()
            .filter(|node| node.stage.primary().info().optional)
            .inspect(|node| {
                assert!(
                    !on_spine.contains(node.stage.primary()),
                    "forked optional {:?} must be off the spine",
                    node.stage.primary(),
                );
            })
            .count();
        assert_eq!(
            optional_off_spine, 6,
            "all six optional stages must fork off"
        );

        let forks = structure
            .nodes
            .keys()
            .filter(|&id| structure.is_fork(id))
            .count();
        let merges = structure
            .nodes
            .keys()
            .filter(|&id| structure.is_merge(id))
            .count();
        assert_eq!(forks, 3, "one fork per optional-stage run");
        assert_eq!(merges, 3, "one reconvergence merge per optional-stage run");
    }

    #[test]
    fn zero_fork_chance_yields_the_linear_trunk() {
        let generator =
            Generator::new(vec![Box::new(BackbonePass::with_config(NarrativeConfig {
                plot: PlotChoice::Seeded,
                fork_chance_permille: 0,
            }))]);
        let world = generator
            .generate_structure(42)
            .expect("backbone-only generation succeeds");
        let structure = &world.story.structure;
        assert_eq!(structure.validate(), Ok(()));
        let spine_stages: Vec<MonomythStage> = structure
            .spine()
            .iter()
            .map(|&id| *structure.node(id).expect("spine node").stage.primary())
            .collect();
        assert!(
            spine_stages
                .iter()
                .copied()
                .eq(MonomythStage::all().iter().copied()),
            "with no forks the spine is the whole 17-stage trunk",
        );
        assert!(
            structure.nodes.values().all(|node| node.out.len() <= 1),
            "with no forks no node branches",
        );
    }

    #[test]
    fn beat_pass_grows_the_structure_well_past_the_bare_arc() {
        let world = generated();
        let structure = &world.story.structure;
        assert_eq!(
            structure.validate(),
            Ok(()),
            "the grown structure must be valid"
        );
        assert!(
            structure.nodes.len() > 25,
            "the beat pass must grow the structure well past the bare arc, got {}",
            structure.nodes.len(),
        );
    }

    #[test]
    fn every_beat_is_grounded_in_its_stage_functions() {
        let world = generated();
        for node in world.story.structure.nodes.values() {
            if !node.label.contains("_beat_") {
                continue;
            }
            let allowed: BTreeSet<_> = arc_functions(*node.stage.primary())
                .iter()
                .copied()
                .collect();
            assert!(
                node.functions
                    .keys()
                    .all(|function| allowed.contains(function)),
                "beat {:?} carries functions outside its stage's arc: {:?}",
                node.label,
                node.functions,
            );
        }
    }

    #[test]
    fn cast_contains_a_hero_and_a_villain() {
        let world = generated();
        assert!(
            world
                .entities
                .values()
                .any(|entity| entity.role.as_ref().map(|role| *role.primary())
                    == Some(ProppRole::Hero)),
            "the cast must include a Hero",
        );
        assert!(
            world
                .entities
                .values()
                .any(|entity| entity.role.as_ref().map(|role| *role.primary())
                    == Some(ProppRole::Villain)),
            "the cast must include a Villain",
        );
    }

    #[test]
    fn entity_location_relation_is_in_sync() {
        let world = generated();
        for (entity_id, entity) in &world.entities {
            let location = entity.location.expect("cast entities are placed");
            assert!(
                world.locations[location].entities.contains(&entity_id),
                "location must list the entity placed there",
            );
        }
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
        for node in world.story.structure.nodes.values() {
            slots.push(&node.synopsis);
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
