//! The Plan phase: a pure, deterministic reduction of a [`World`]'s narrative
//! spine to an [`Outline`].
//!
//! Plan is the first stage of the long-form compose pipeline. The later Draft
//! phase narrates the outline this phase produces; the LLM narrates a structure it
//! cannot invent — [`plan`] never touches an LLM, never performs IO, and is a pure
//! function of `world`.

use monomyth_core::World;

use crate::error::ComposeError;
use crate::outline::{Outline, OutlineSection};

/// Reduce `world`'s narrative spine to an [`Outline`].
///
/// Walks [`world.story.structure.spine()`](monomyth_core::NarrativeStructure::spine)
/// in order; for each [`NarrativeNodeId`](monomyth_core::NarrativeNodeId) that
/// resolves to a real node, extracts its primary
/// [`MonomythStage`](monomyth_frameworks::MonomythStage) and its synopsis
/// prompt's hint into an [`OutlineSection`]. A spine id that does not resolve to a
/// node is skipped rather than treated as fatal: [`NarrativeStructure::spine`]
/// walks from `root` regardless of whether `root` is actually present in `nodes`
/// (an empty/default structure has a spine of length one holding a dangling root
/// id), so a non-resolving id is the structural signature of "no real content"
/// rather than an invariant violation to panic on.
///
/// # Errors
///
/// Returns [`ComposeError::EmptyOutline`] if no spine id resolves to a node, i.e.
/// the world has nothing composable.
pub fn plan(world: &World) -> Result<Outline, ComposeError> {
    let structure = &world.story.structure;
    let sections: Vec<OutlineSection> = structure
        .spine()
        .into_iter()
        .filter_map(|node_id| structure.node(node_id).map(|node| (node_id, node)))
        .map(|(node_id, node)| OutlineSection {
            node_id,
            stage: *node.stage.primary(),
            synopsis_hint: node.synopsis.prompt().hint.clone(),
        })
        .collect();

    if sections.is_empty() {
        return Err(ComposeError::EmptyOutline);
    }

    Ok(Outline { sections })
}

#[cfg(test)]
mod tests {
    use monomyth_core::{
        Content, ContentKind, ContentPrompt, LocationId, NarrativeStructure, Player, RngState,
        SCHEMA_VERSION, Story, World, WorldMeta, WorldState,
    };
    use monomyth_gen::Generator;

    use super::plan;
    use crate::error::ComposeError;

    /// The seed used across the plan tests; arbitrary but fixed for determinism.
    const SEED: u64 = 42;

    /// A `World` with an empty (default) narrative structure: no nodes, so the
    /// spine, once resolved, has nothing composable.
    ///
    /// `NarrativeStructure::default()` sets `root` to the default (null)
    /// `NarrativeNodeId`, which is not present in the empty `nodes` map — so
    /// `spine()` returns a one-element vec holding that dangling id rather than an
    /// empty vec. Hand-constructing a `NarrativeStructure` with a genuinely empty
    /// `spine()` isn't possible through the public API (there is no way to build a
    /// structure with zero nodes and a resolving root), so this is the smallest
    /// legitimate "nothing composable" world: `plan` must treat a spine that
    /// resolves to zero nodes as the empty case.
    fn world_with_empty_structure() -> World {
        World {
            meta: WorldMeta {
                seed: SEED,
                schema_version: SCHEMA_VERSION,
                title: Content::empty(ContentPrompt::new(ContentKind::Title, "untitled")),
            },
            locations: empty_slotmap(),
            entities: empty_slotmap(),
            items: empty_slotmap(),
            player: Player::new(LocationId::default()),
            story: Story {
                structure: NarrativeStructure::default(),
                plot: None,
                quests: empty_slotmap(),
            },
            state: WorldState::default(),
            rng: RngState::new(SEED),
        }
    }

    /// An empty typed slotmap, inferred from the field it initializes.
    ///
    /// Avoids naming the `slotmap` crate directly in this dev-only test helper
    /// (it is a transitive dependency through the model, not a declared one here).
    fn empty_slotmap<T: Default>() -> T {
        T::default()
    }

    #[test]
    fn should_produce_outline_matching_spine_for_seeded_world() {
        let world = Generator::with_default_passes()
            .generate_structure(SEED)
            .expect("the default pipeline generates a world");

        let outline = plan(&world).expect("a generated world is composable");

        let spine = world.story.structure.spine();
        assert_eq!(outline.sections().len(), spine.len());

        let outline_ids: Vec<_> = outline
            .sections()
            .iter()
            .map(|section| section.node_id)
            .collect();
        assert_eq!(outline_ids, spine);

        for section in outline.sections() {
            let node = world
                .story
                .structure
                .node(section.node_id)
                .expect("outline node id resolves in the source world");
            assert_eq!(section.stage, *node.stage.primary());
            assert_eq!(section.synopsis_hint, node.synopsis.prompt().hint);
        }
    }

    #[test]
    fn should_be_deterministic_across_repeated_plan_calls() {
        let world = Generator::with_default_passes()
            .generate_structure(SEED)
            .expect("the default pipeline generates a world");

        let first = plan(&world).expect("first plan call succeeds");
        let second = plan(&world).expect("second plan call succeeds");

        let first_json = serde_json::to_string(&first).expect("outline serializes");
        let second_json = serde_json::to_string(&second).expect("outline serializes");
        assert_eq!(first_json, second_json);
    }

    #[test]
    fn should_return_empty_outline_error_when_spine_is_empty() {
        let world = world_with_empty_structure();
        assert!(
            matches!(plan(&world), Err(ComposeError::EmptyOutline)),
            "an empty spine must fail with ComposeError::EmptyOutline"
        );
    }
}
