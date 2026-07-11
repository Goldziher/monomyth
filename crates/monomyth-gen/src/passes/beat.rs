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
//! - a subset of [`arc_functions`](monomyth_frameworks::arc_functions) (Propp),
//! - one [`plot_situations`](monomyth_frameworks::plot_situations) draw (Polti),
//!   when the story records a [`BookerPlot`](monomyth_frameworks::BookerPlot),
//! - a small subset of [`stage_motif_candidates`] (Thompson's motif classes).
//!
//! Prose slots (node synopsis) stay empty for the later content layer.

use std::collections::BTreeSet;

use monomyth_core::{EdgeKind, EditOutcome, NarrativeEdit, NarrativeNodeId, NodeSpec, World};
use monomyth_frameworks::{MonomythStage, MotifClass, arc_functions, plot_situations};
use rand_chacha::ChaCha8Rng;

use crate::error::GenError;
use crate::pass::{ProceduralPass, draw_range_inclusive};

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
        let situations = world.story.plot.map(plot_situations).unwrap_or_default();

        for source in original_ids {
            let Some((stage, target)) = spine_edge(world, source) else {
                continue;
            };

            let beat_count = draw_range_inclusive(
                rng,
                self.config.beats_per_stage_min,
                self.config.beats_per_stage_max,
            );
            let functions = arc_functions(stage);
            let motif_candidates = stage_motif_candidates(stage);

            let mut previous = source;
            for beat_index in 0..beat_count {
                let ordinal = beat_index + 1;
                let hint = format!("beat {ordinal} expanding the {stage:?} stage");
                let mut spec = NodeSpec::new(format!("{stage:?}_beat_{ordinal}"), stage, hint);
                spec.functions = draw_function_subset(rng, functions);
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

/// Draw a distinct, deterministic subset of `candidates` of size `[min, max]`
/// (clamped to `candidates.len()`).
///
/// Repeatedly draws a random index into the shrinking pool of not-yet-chosen
/// candidate indices and removes it (a partial Fisher-Yates), so the draws
/// consume randomness only for the elements actually picked. `ProppFunction` and
/// `MotifClass` both derive `Ord` from [`framework_enum!`](monomyth_frameworks),
/// whose variant declaration order matches the artifact's canonical numeric `id`
/// order; collecting the chosen elements into a `BTreeSet` therefore recovers
/// canonical order by construction; the *selection* need not preserve order.
fn draw_indexed_subset(rng: &mut ChaCha8Rng, len: usize, min: usize, max: usize) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    let max = max.min(len);
    let min = min.min(max);
    let count = draw_range_inclusive(rng, min, max);

    let mut pool: Vec<usize> = (0..len).collect();
    let mut chosen = Vec::with_capacity(count);
    for _ in 0..count {
        let pick = draw_range_inclusive(rng, 0, pool.len() - 1);
        chosen.push(pool.swap_remove(pick));
    }
    chosen
}

/// Draw a non-empty, canonically-ordered subset of `functions` (at least one when
/// `functions` is non-empty), grounding the beat in a plausible slice of the
/// stage's Propp functions rather than inventing a single arbitrary pick.
fn draw_function_subset(
    rng: &mut ChaCha8Rng,
    functions: &'static [monomyth_frameworks::ProppFunction],
) -> BTreeSet<monomyth_frameworks::ProppFunction> {
    let indices = draw_indexed_subset(rng, functions.len(), 1, functions.len());
    indices.into_iter().map(|index| functions[index]).collect()
}

/// Draw at most one Polti situation from `situations` (empty when the story has no
/// recorded plot or the plot has no crosswalk entries).
fn draw_situation(
    rng: &mut ChaCha8Rng,
    situations: &'static [monomyth_frameworks::PoltiSituation],
) -> Option<monomyth_frameworks::PoltiSituation> {
    if situations.is_empty() {
        return None;
    }
    situations
        .get(draw_range_inclusive(rng, 0, situations.len() - 1))
        .copied()
}

/// Draw a `0..=2`-sized, canonically-ordered subset of `candidates` (see
/// [`MOTIFS_PER_BEAT_MIN`]/[`MOTIFS_PER_BEAT_MAX`]).
fn draw_motif_subset(
    rng: &mut ChaCha8Rng,
    candidates: &'static [MotifClass],
) -> BTreeSet<MotifClass> {
    let indices = draw_indexed_subset(
        rng,
        candidates.len(),
        MOTIFS_PER_BEAT_MIN,
        MOTIFS_PER_BEAT_MAX,
    );
    indices.into_iter().map(|index| candidates[index]).collect()
}

/// The Campbell stage of `source` and the target of its primary spine edge (the first
/// [`EdgeKind::Sequence`] out-edge), or `None` if `source` is unknown or terminal.
fn spine_edge(world: &World, source: NarrativeNodeId) -> Option<(MonomythStage, NarrativeNodeId)> {
    let node = world.story.structure.node(source)?;
    let edge = node
        .out
        .iter()
        .find(|edge| edge.kind == EdgeKind::Sequence)?;
    Some((node.stage, edge.target))
}

/// The Thompson motif classes a beat on `stage` may plausibly realize.
///
/// No crosswalk artifact binds Campbell stages to Thompson motif classes (the
/// framework corpus stops at Propp/Polti for the meso tier), so this mapping is a
/// hand-authored, deterministic pairing grounded in Thompson's own class glosses
/// (see `artifacts/frameworks/thompson_motif_classes.json`) rather than an
/// invented taxonomy. Every one of the seventeen [`MonomythStage`] variants maps
/// to at least one candidate; a beat may still realize zero motifs if the seeded
/// subset draw comes up empty.
///
/// | Stage | Candidates | Why |
/// |---|---|---|
/// | [`CallToAdventure`](MonomythStage::CallToAdventure) | [`OrdainingTheFuture`](MotifClass::OrdainingTheFuture), [`ChanceAndFate`](MotifClass::ChanceAndFate) | the disrupting summons reads as a prophecy/fate-class motif |
/// | [`RefusalOfTheCall`](MonomythStage::RefusalOfTheCall) | [`TraitsOfCharacter`](MotifClass::TraitsOfCharacter) | hesitation out of fear or duty is a personality-trait beat |
/// | [`SupernaturalAid`](MonomythStage::SupernaturalAid) | [`Magic`](MotifClass::Magic) | the mentor's talisman/spell is Thompson's own "magic objects" class |
/// | [`CrossingTheFirstThreshold`](MonomythStage::CrossingTheFirstThreshold) | [`Marvels`](MotifClass::Marvels) | passage into the unknown is an otherworld-journey motif |
/// | [`BellyOfTheWhale`](MonomythStage::BellyOfTheWhale) | [`TheDead`](MotifClass::TheDead) | symbolic death/passage matches "resuscitation... and the soul" |
/// | [`TheRoadOfTrials`](MonomythStage::TheRoadOfTrials) | [`Tests`](MotifClass::Tests) | a direct match: "tests of identity, cleverness, and prowess; quests" |
/// | [`TheMeetingWithTheGoddess`](MonomythStage::TheMeetingWithTheGoddess) | [`Sex`](MotifClass::Sex) | unconditional love matches "love, marriage, courtship" |
/// | [`WomanAsTemptress`](MonomythStage::WomanAsTemptress) | [`Deceptions`](MotifClass::Deceptions) | temptation leading the hero astray is "deceits... disguises, and illusions" |
/// | [`AtonementWithTheFather`](MonomythStage::AtonementWithTheFather) | [`Society`](MotifClass::Society) | the ultimate authority figure matches "kings, courts... institutions" |
/// | [`Apotheosis`](MonomythStage::Apotheosis) | [`Religion`](MotifClass::Religion) | divine knowledge/transcendence is a direct match |
/// | [`TheUltimateBoon`](MonomythStage::TheUltimateBoon) | [`RewardsAndPunishments`](MotifClass::RewardsAndPunishments) | winning the sought prize is a reward motif |
/// | [`RefusalOfTheReturn`](MonomythStage::RefusalOfTheReturn) | [`ChanceAndFate`](MotifClass::ChanceAndFate) | resisting the pull back to the ordinary world is a fate/fortune motif |
/// | [`TheMagicFlight`](MonomythStage::TheMagicFlight) | [`CaptivesAndFugitives`](MotifClass::CaptivesAndFugitives) | a direct match: "escape, pursuit, and rescue" |
/// | [`RescueFromWithout`](MonomythStage::RescueFromWithout) | [`CaptivesAndFugitives`](MotifClass::CaptivesAndFugitives) | the same class's "rescue" half |
/// | [`TheCrossingOfTheReturnThreshold`](MonomythStage::TheCrossingOfTheReturnThreshold) | [`ReversalOfFortune`](MotifClass::ReversalOfFortune) | re-entry into ordinary life is a fortune-state change |
/// | [`MasterOfTheTwoWorlds`](MonomythStage::MasterOfTheTwoWorlds) | [`TheWiseAndTheFoolish`](MotifClass::TheWiseAndTheFoolish) | balance/mastery matches the "wisdom, cleverness" class |
/// | [`FreedomToLive`](MonomythStage::FreedomToLive) | [`TheNatureOfLife`](MotifClass::TheNatureOfLife) | a direct match: "reflective observations on the way of the world" |
#[must_use]
fn stage_motif_candidates(stage: MonomythStage) -> &'static [MotifClass] {
    use MotifClass::{
        CaptivesAndFugitives, ChanceAndFate, Deceptions, Magic, Marvels, OrdainingTheFuture,
        Religion, ReversalOfFortune, RewardsAndPunishments, Sex, Society, Tests, TheDead,
        TheNatureOfLife, TheWiseAndTheFoolish, TraitsOfCharacter,
    };

    match stage {
        MonomythStage::CallToAdventure => &[OrdainingTheFuture, ChanceAndFate],
        MonomythStage::RefusalOfTheCall => &[TraitsOfCharacter],
        MonomythStage::SupernaturalAid => &[Magic],
        MonomythStage::CrossingTheFirstThreshold => &[Marvels],
        MonomythStage::BellyOfTheWhale => &[TheDead],
        MonomythStage::TheRoadOfTrials => &[Tests],
        MonomythStage::TheMeetingWithTheGoddess => &[Sex],
        MonomythStage::WomanAsTemptress => &[Deceptions],
        MonomythStage::AtonementWithTheFather => &[Society],
        MonomythStage::Apotheosis => &[Religion],
        MonomythStage::TheUltimateBoon => &[RewardsAndPunishments],
        MonomythStage::RefusalOfTheReturn => &[ChanceAndFate],
        MonomythStage::TheMagicFlight | MonomythStage::RescueFromWithout => &[CaptivesAndFugitives],
        MonomythStage::TheCrossingOfTheReturnThreshold => &[ReversalOfFortune],
        MonomythStage::MasterOfTheTwoWorlds => &[TheWiseAndTheFoolish],
        MonomythStage::FreedomToLive => &[TheNatureOfLife],
    }
}

#[cfg(test)]
mod tests {
    use monomyth_core::doc_support::single_room_world;
    use monomyth_frameworks::{MonomythStage, arc_functions, plot_situations};
    use rand::SeedableRng;

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
            let allowed = arc_functions(beat.stage);
            if allowed.is_empty() {
                continue;
            }
            assert!(
                !beat.functions.is_empty(),
                "beat {:?} on stage {:?} with non-empty arc_functions must realize at least one",
                beat.label,
                beat.stage,
            );
            saw_nonempty = true;

            let realized: Vec<_> = beat.functions.iter().copied().collect();
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
                    .iter()
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
        let plot = world.story.plot.expect("backbone pass records a plot");
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
                .is_some_and(|situation| allowed.contains(&situation))),
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
                !stage_motif_candidates(stage).is_empty(),
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
            let candidates: BTreeSet<_> =
                stage_motif_candidates(beat.stage).iter().copied().collect();
            assert!(
                beat.motifs.iter().all(|motif| candidates.contains(motif)),
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
