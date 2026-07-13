//! [`BeatPass`]: grow the narrative to a playable length by expanding each Campbell
//! stage node into a short chain of grounded beats.
//!
//! This is the **meso length** tier of narrative generation, sitting between the
//! macro [`BackbonePass`](crate::BackbonePass) (one node per stage) and the future
//! micro content layer. The backbone lays down a spine of roughly a dozen-and-a-half
//! stage nodes; a real adventure needs more moment-to-moment structure than that.
//! This pass walks every existing node and splices a seed-chosen number of extra
//! beats onto its primary spine (`Sequence`) edge, so a single stage becomes
//! `stage → beat_1 → … → beat_k → next`.
//!
//! The growth is driven entirely through the narrative *edit surface*
//! ([`NarrativeEdit::InsertBeat`]): the pass never touches the [`SlotMap`] or edge
//! vectors directly, so every mutation is an inspectable, validated command. Each new
//! beat inherits its parent's Campbell [`stage`](monomyth_frameworks::MonomythStage)
//! and is grounded in the scholarship, not invention:
//!
//! - a weighted subset of [`arc_functions_weighted`](monomyth_frameworks::arc_functions_weighted) (Propp),
//! - one weighted [`plot_situations_weighted`](monomyth_frameworks::plot_situations_weighted) draw (Polti),
//!   when the story records a [`BookerPlot`](monomyth_frameworks::BookerPlot),
//! - a small weighted subset of [`stage_motifs_weighted`](monomyth_frameworks::stage_motifs_weighted)
//!   (Thompson's motif classes).
//!
//! Prose slots (node synopsis) stay empty for the later content layer.

use monomyth_core::{
    EdgeKind, EditOutcome, NarrativeEdit, NarrativeNodeId, NodeSpec, ScoredOne, ScoredSet, Weight,
    World,
};
use monomyth_frameworks::{
    MonomythStage, MotifClass, arc_functions_weighted, plot_situations_weighted,
    stage_motifs_weighted,
};
use rand_chacha::ChaCha8Rng;

use crate::error::GenError;
use crate::pass::{ProceduralPass, draw_range_inclusive, draw_weighted_index};

/// The fewest beats the pass splices onto each stage node's spine.
const DEFAULT_BEATS_MIN: usize = 1;
/// The most beats the pass splices onto each stage node's spine.
const DEFAULT_BEATS_MAX: usize = 3;

/// The fewest Thompson motif classes realized per beat.
const MOTIFS_PER_BEAT_MIN: usize = 0;
/// The most Thompson motif classes realized per beat.
const MOTIFS_PER_BEAT_MAX: usize = 2;

/// Tunable knobs for beat expansion (the meso length layer).
///
/// A stage node is expanded into `beats_per_stage_min..=beats_per_stage_max` extra
/// beats, drawn per node from the pass's sub-stream. The invariant `min <= max` must
/// hold; [`draw_range_inclusive`] debug-asserts it.
#[derive(Debug, Clone, Copy)]
pub struct BeatConfig {
    /// The fewest beats to splice onto each stage node (must be `<= max`).
    pub beats_per_stage_min: usize,
    /// The most beats to splice onto each stage node (must be `>= min`).
    pub beats_per_stage_max: usize,
}

impl Default for BeatConfig {
    fn default() -> Self {
        Self {
            beats_per_stage_min: DEFAULT_BEATS_MIN,
            beats_per_stage_max: DEFAULT_BEATS_MAX,
        }
    }
}

/// Expands each existing narrative node into a short chain of grounded beats, growing
/// the structure to a playable length through the edit surface.
#[derive(Debug, Default, Clone, Copy)]
pub struct BeatPass {
    config: BeatConfig,
}

impl BeatPass {
    /// Construct the beat pass with the default [`BeatConfig`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct the beat pass with an explicit configuration.
    #[must_use]
    pub fn with_config(config: BeatConfig) -> Self {
        Self { config }
    }
}

impl ProceduralPass for BeatPass {
    fn name(&self) -> &'static str {
        "BeatPass"
    }

    fn apply(&self, world: &mut World, rng: &mut ChaCha8Rng) -> Result<(), GenError> {
        let mut original_ids: Vec<NarrativeNodeId> = world.story.structure.nodes.keys().collect();
        original_ids.sort_unstable();
        let situations = world
            .story
            .plot
            .as_ref()
            .map(|plot| plot_situations_weighted(*plot.primary()))
            .unwrap_or_default();

        for source in original_ids {
            let Some((stage, target)) = spine_edge(world, source) else {
                continue;
            };

            let beat_count = draw_range_inclusive(
                rng,
                self.config.beats_per_stage_min,
                self.config.beats_per_stage_max,
            );
            let motif_candidates = stage_motifs_weighted(stage);

            let mut previous = source;
            for beat_index in 0..beat_count {
                let ordinal = beat_index + 1;
                let hint = format!("beat {ordinal} expanding the {stage:?} stage");
                let mut spec = NodeSpec::new(format!("{stage:?}_beat_{ordinal}"), stage, hint);
                spec.functions = draw_function_subset(rng, stage);
                spec.situation = draw_situation(rng, situations);
                spec.motifs = draw_motif_subset(rng, motif_candidates);

                let edit = NarrativeEdit::InsertBeat {
                    source: previous,
                    target,
                    spec,
                };
                let EditOutcome::NodeAdded(new_id) = world.story.structure.apply_edit(&edit)?
                else {
                    return Err(GenError::Invariant(
                        "InsertBeat must report the new node id",
                    ));
                };
                previous = new_id;
            }
        }

        world.story.structure.recompute_kinds();
        world.story.structure.validate()?;
        Ok(())
    }
}

/// Draw a non-empty, canonically-ordered, weighted subset of `stage`'s crosswalk
/// Propp functions (at least one when the stage has any candidates), grounding the
/// beat in a plausible slice of the stage's arc rather than inventing a single
/// arbitrary pick.
///
/// Selection is weighted-without-replacement: each of `count` picks draws an index
/// into the *remaining* candidate pool proportional to its crosswalk weight (via
/// [`draw_weighted_index`]), and the chosen candidate is removed from the pool
/// before the next pick (mirroring [`draw_indexed_subset`]'s partial Fisher-Yates,
/// but weighted rather than uniform). The result is a [`ScoredSet`], so canonical
/// order falls out of its `BTreeMap` backing with no separate sort needed.
fn draw_function_subset(
    rng: &mut ChaCha8Rng,
    stage: MonomythStage,
) -> ScoredSet<monomyth_frameworks::ProppFunction> {
    let candidates = arc_functions_weighted(stage);
    if candidates.is_empty() {
        return ScoredSet::new();
    }

    let count = draw_range_inclusive(rng, 1, candidates.len());
    let mut pool: Vec<(monomyth_frameworks::ProppFunction, Weight)> = candidates
        .iter()
        .map(|&(function, permille)| (function, Weight::new(permille)))
        .collect();

    let mut chosen = ScoredSet::new();
    for _ in 0..count {
        let weights: Vec<Weight> = pool.iter().map(|&(_, weight)| weight).collect();
        let Some(index) = draw_weighted_index(rng, &weights) else {
            break;
        };
        let (function, weight) = pool.swap_remove(index);
        chosen.insert(function, weight);
    }
    chosen
}

/// Draw at most one weighted Polti situation from `situations` (empty when the
/// story has no recorded plot or the plot has no crosswalk entries), wrapped as a
/// [`ScoredOne`] with no alternatives.
fn draw_situation(
    rng: &mut ChaCha8Rng,
    situations: &'static [(monomyth_frameworks::PoltiSituation, u16)],
) -> Option<ScoredOne<monomyth_frameworks::PoltiSituation>> {
    if situations.is_empty() {
        return None;
    }
    let weights: Vec<Weight> = situations
        .iter()
        .map(|&(_, permille)| Weight::new(permille))
        .collect();
    let index = draw_weighted_index(rng, &weights)?;
    Some(ScoredOne::new(situations[index].0))
}

/// Draw a `0..=2`-sized, canonically-ordered, weighted subset of `candidates` (see
/// [`MOTIFS_PER_BEAT_MIN`]/[`MOTIFS_PER_BEAT_MAX`]).
///
/// Selection is weighted-without-replacement, mirroring
/// [`draw_function_subset`]'s pool-and-`swap_remove` pattern, but preserves this
/// field's own `0..=2` count semantics (unlike `draw_function_subset`'s
/// always-at-least-one count): the count is drawn from
/// `MOTIFS_PER_BEAT_MIN..=MOTIFS_PER_BEAT_MAX`, clamped to `candidates.len()`
/// exactly as the former uniform `draw_indexed_subset` clamped it.
fn draw_motif_subset(
    rng: &mut ChaCha8Rng,
    candidates: &'static [(MotifClass, u16)],
) -> ScoredSet<MotifClass> {
    if candidates.is_empty() {
        return ScoredSet::new();
    }

    let max = MOTIFS_PER_BEAT_MAX.min(candidates.len());
    // MOTIFS_PER_BEAT_MIN is 0, which is never greater than `max`, so no clamp is ~keep
    // needed here (unlike the general min/max clamp `draw_indexed_subset` used to ~keep
    // do for its caller-supplied bounds); clippy's `unnecessary_min_or_max` lint ~keep
    // catches this statically. ~keep
    let count = draw_range_inclusive(rng, MOTIFS_PER_BEAT_MIN, max);

    let mut pool: Vec<(MotifClass, Weight)> = candidates
        .iter()
        .map(|&(motif, permille)| (motif, Weight::new(permille)))
        .collect();

    let mut chosen = ScoredSet::new();
    for _ in 0..count {
        let weights: Vec<Weight> = pool.iter().map(|&(_, weight)| weight).collect();
        let Some(index) = draw_weighted_index(rng, &weights) else {
            break;
        };
        let (motif, weight) = pool.swap_remove(index);
        chosen.insert(motif, weight);
    }
    chosen
}

/// The Campbell stage of `source` and the target of its primary spine edge (the first
/// [`EdgeKind::Sequence`] out-edge), or `None` if `source` is unknown or terminal.
fn spine_edge(world: &World, source: NarrativeNodeId) -> Option<(MonomythStage, NarrativeNodeId)> {
    let node = world.story.structure.node(source)?;
    let edge = node
        .out
        .iter()
        .find(|edge| edge.kind == EdgeKind::Sequence)?;
    Some((*node.stage.primary(), edge.target))
}

#[cfg(test)]
mod tests {
    use monomyth_core::doc_support::single_room_world;
    use monomyth_frameworks::{MonomythStage, arc_functions, plot_situations};
    use rand::SeedableRng;
    use std::collections::BTreeSet;

    use super::*;
    use crate::passes::backbone::BackbonePass;

    /// A world with a real backbone (so `story.plot` is recorded) grown from `seed`.
    fn world_with_backbone(seed: u64) -> World {
        let mut world = single_room_world();
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        BackbonePass::new()
            .apply(&mut world, &mut rng)
            .expect("backbone pass succeeds");
        world
    }

    fn beats(world: &World) -> Vec<monomyth_core::NarrativeNode> {
        world
            .story
            .structure
            .nodes
            .values()
            .filter(|node| node.label.contains("_beat_"))
            .cloned()
            .collect()
    }

    #[test]
    fn should_set_nonempty_functions_in_canonical_order_when_stage_has_functions() {
        let mut world = world_with_backbone(42);
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        BeatPass::new()
            .apply(&mut world, &mut rng)
            .expect("beat pass succeeds");

        let mut saw_nonempty = false;
        for beat in beats(&world) {
            let allowed = arc_functions(*beat.stage.primary());
            if allowed.is_empty() {
                continue;
            }
            assert!(
                !beat.functions.is_empty(),
                "beat {:?} on stage {:?} with non-empty arc_functions must realize at least one",
                beat.label,
                beat.stage.primary(),
            );
            saw_nonempty = true;

            let realized: Vec<_> = beat.functions.keys().copied().collect();
            let mut expected_order: Vec<_> = realized.clone();
            expected_order.sort_by_key(|function| {
                allowed
                    .iter()
                    .position(|candidate| candidate == function)
                    .expect("realized function is drawn from arc_functions")
            });
            assert_eq!(
                realized, expected_order,
                "realized functions for beat {:?} must preserve canonical Propp order",
                beat.label,
            );
            assert!(
                beat.functions
                    .keys()
                    .all(|function| allowed.contains(function)),
                "beat {:?} carries a function outside its stage's arc",
                beat.label,
            );
        }
        assert!(
            saw_nonempty,
            "at least one beat must have a non-empty stage arc to exercise the assertion",
        );
    }

    #[test]
    fn should_set_situation_when_plot_is_recorded_and_has_situations() {
        let mut world = world_with_backbone(42);
        let plot = *world
            .story
            .plot
            .as_ref()
            .expect("backbone pass records a plot")
            .primary();
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        BeatPass::new()
            .apply(&mut world, &mut rng)
            .expect("beat pass succeeds");

        if plot_situations(plot).is_empty() {
            return;
        }
        let allowed: BTreeSet<_> = plot_situations(plot).iter().copied().collect();
        assert!(
            beats(&world).iter().any(|beat| beat
                .situation
                .as_ref()
                .is_some_and(|situation| allowed.contains(situation.primary()))),
            "at least one beat must realize a situation drawn from the plot's Polti situations",
        );
    }

    #[test]
    fn should_leave_situation_none_when_no_plot_is_recorded() {
        let mut world = single_room_world();
        assert!(world.story.plot.is_none());
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        BeatPass::new()
            .apply(&mut world, &mut rng)
            .expect("beat pass succeeds even with no recorded plot");

        assert!(
            beats(&world).iter().all(|beat| beat.situation.is_none()),
            "with no recorded plot every beat's situation must stay None",
        );
    }

    #[test]
    fn should_map_every_monomyth_stage_to_at_least_one_motif_candidate() {
        for &stage in MonomythStage::all() {
            assert!(
                !stage_motifs_weighted(stage).is_empty(),
                "stage {stage:?} must map to at least one MotifClass candidate",
            );
        }
    }

    #[test]
    fn should_set_motifs_from_the_stage_candidate_list() {
        let mut world = world_with_backbone(42);
        let mut rng = ChaCha8Rng::seed_from_u64(7);
        BeatPass::new()
            .apply(&mut world, &mut rng)
            .expect("beat pass succeeds");

        for beat in beats(&world) {
            let candidates: BTreeSet<_> = stage_motifs_weighted(*beat.stage.primary())
                .iter()
                .map(|&(motif, _)| motif)
                .collect();
            assert!(
                beat.motifs.keys().all(|motif| candidates.contains(motif)),
                "beat {:?} carries a motif outside its stage's candidate list",
                beat.label,
            );
        }
    }

    #[test]
    fn should_be_deterministic_for_the_same_seed() {
        let world_a = {
            let mut world = world_with_backbone(42);
            let mut rng = ChaCha8Rng::seed_from_u64(7);
            BeatPass::new()
                .apply(&mut world, &mut rng)
                .expect("beat pass succeeds");
            world
        };
        let world_b = {
            let mut world = world_with_backbone(42);
            let mut rng = ChaCha8Rng::seed_from_u64(7);
            BeatPass::new()
                .apply(&mut world, &mut rng)
                .expect("beat pass succeeds");
            world
        };

        let serialized_a = serde_json::to_string(&world_a).expect("world serializes");
        let serialized_b = serde_json::to_string(&world_b).expect("world serializes");
        assert_eq!(
            serialized_a, serialized_b,
            "the same seed pair must reproduce byte-identical beat-expanded worlds",
        );
    }
}
