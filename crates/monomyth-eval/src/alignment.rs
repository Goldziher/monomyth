//! [`AlignmentScorer`] — a concrete node-alignment [`Scorer`] (ADR-0023 Phase B3).
//!
//! Node alignment is the sharpest-risk piece of the eval harness: every
//! downstream per-node metric depends on matching the right gold node to the
//! right predicted node, and a wrong match silently corrupts everything built on
//! top of it. This module is deliberately narrow and heavily unit-tested against
//! hand-crafted near-miss cases (ADR-0023's Decision Outcome) rather than reused
//! from a general-purpose alignment library, so its exact behavior on a
//! relabeled node, a dropped node, or a swap is pinned and reviewable.
//!
//! # Scope: the primary spine only
//!
//! [`AlignmentScorer`] linearizes both the gold and predicted
//! [`NarrativeStructure`](monomyth_core::NarrativeStructure) via
//! [`spine`](monomyth_core::NarrativeStructure::spine) before aligning — a single
//! root-to-ending walk following each node's primary out-edge. **This does not
//! handle branching DAGs**: a fork's non-primary branches, and any reconvergence
//! structure beyond the one spine, are invisible to this aligner. That is not a
//! regression today (every fixture that exists, including the Odyssey benchmark,
//! is a linear chain with no forks), but it is a known, explicit limitation —
//! extending alignment to score forked branches is future work, not silently
//! subsumed by this implementation.

use std::collections::{BTreeMap, BTreeSet};

use monomyth_core::{NarrativeNodeId, World};

use crate::dist::Dist;
use crate::metrics::{DistScore, histogram_intersection};
use crate::report::{Alignment, Report, Scorer};

/// The cost of leaving a gold or predicted node unmatched (an insertion or
/// deletion in the Needleman-Wunsch alignment).
///
/// A substitution's cost is `1.0 - histogram_intersection(gold_stage,
/// predicted_stage)`, itself bounded to `0.0..=1.0`. Setting the gap penalty to
/// `1.0` — exactly the worst possible substitution cost — means a gap never
/// costs *less* than the worst substitution, so the DP is only ever indifferent
/// between "match" and "gap" in the total-mismatch case
/// (`histogram_intersection == 0.0`), and strictly prefers matching the moment
/// there is any partial overlap (`histogram_intersection > 0.0`, so
/// `substitution_cost < GAP_PENALTY`). This is what keeps a relabeled-but-
/// still-partially-plausible node aligned rather than dropped as a gap, while
/// still letting a genuinely extra or missing node surface as a gap instead of
/// being forced into a nonsensical match.
const GAP_PENALTY: f64 = 1.0;

/// One DP cell's minimal-cost predecessor, recorded for backtracking.
///
/// Named separately from a bare cost comparison so the backtrack's tie-break
/// order (see [`AlignmentScorer::align`]'s doc) is a fixed table lookup, not
/// re-derived from float comparisons at backtrack time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// Came from `(i-1, j-1)`: gold node `i-1` matched to predicted node `j-1`.
    Diagonal,
    /// Came from `(i-1, j)`: gold node `i-1` is unmatched (a gap on the gold
    /// side — predicted has, relative to this position, an extra node).
    GoldGap,
    /// Came from `(i, j-1)`: predicted node `j-1` is unmatched (a gap on the
    /// predicted side — gold has, relative to this position, an extra node).
    PredictedGap,
}

/// A concrete node-alignment [`Scorer`].
///
/// Stateless (no configuration knobs yet), so it is a unit struct; see the
/// module doc for the DP recurrence and the spine-only scope limitation.
#[derive(Debug, Clone, Copy, Default)]
pub struct AlignmentScorer;

impl AlignmentScorer {
    /// Needleman-Wunsch global alignment of two already-linearized (fabula
    /// order, via [`spine`](monomyth_core::NarrativeStructure::spine)) node id
    /// sequences.
    ///
    /// # DP recurrence
    ///
    /// Let `gold` have length `m` and `predicted` length `n`. `cost[i][j]` is
    /// the minimal total alignment cost of `gold[..i]` against `predicted[..j]`:
    ///
    /// - `cost[0][0] = 0.0`
    /// - `cost[i][0] = i as f64 * GAP_PENALTY` (every gold node up to `i` unmatched)
    /// - `cost[0][j] = j as f64 * GAP_PENALTY` (every predicted node up to `j` unmatched)
    /// - `cost[i][j] = min(`
    ///   `cost[i-1][j-1] + substitution_cost(gold[i-1], predicted[j-1])`,
    ///   `cost[i-1][j] + GAP_PENALTY`,
    ///   `cost[i][j-1] + GAP_PENALTY`
    ///   `)`
    ///
    /// where `substitution_cost(g, p) = 1.0 - histogram_intersection(&Dist::from(&g.stage),
    /// &Dist::from(&p.stage))`, always in `0.0..=1.0`.
    ///
    /// # Tie-break rule (backtracking)
    ///
    /// When two or more of the three predecessor costs tie exactly, this
    /// function prefers, in order: **diagonal** (substitution/match) over
    /// either gap, then **gold-gap** over **predicted-gap**. Diagonal-first
    /// means a same-cost match is never sacrificed for an equally-costed gap,
    /// keeping the alignment maximally informative. The gold-gap-before-
    /// predicted-gap order is an arbitrary but fixed total order (either is
    /// equally valid on a genuine tie) chosen once here so the result never
    /// depends on iteration order. Every comparison used to pick a predecessor
    /// is a plain `f64` `<=` on a small sum of bounded terms (DP costs are sums
    /// of at most `m + n` terms each in `0.0..=1.0`) — the tie-break itself
    /// never relies on float-equality comparison, only this fixed preference
    /// order applied when `<=` holds in more than one direction.
    fn align(
        gold: &[NarrativeNodeId],
        predicted: &[NarrativeNodeId],
        gold_structure: &monomyth_core::NarrativeStructure,
        predicted_structure: &monomyth_core::NarrativeStructure,
    ) -> Alignment {
        let rows = gold.len() + 1;
        let cols = predicted.len() + 1;
        let mut cost = vec![vec![0.0_f64; cols]; rows];
        let mut step = vec![vec![Step::Diagonal; cols]; rows];

        for (row, cost_row) in cost.iter_mut().enumerate().skip(1) {
            #[allow(
                clippy::cast_precision_loss,
                reason = "spine lengths are tiny (bounded by narrative node counts); precision loss is not reachable"
            )]
            let gap_cost = row as f64 * GAP_PENALTY;
            cost_row[0] = gap_cost;
            step[row][0] = Step::GoldGap;
        }
        for col in 1..cols {
            #[allow(
                clippy::cast_precision_loss,
                reason = "spine lengths are tiny (bounded by narrative node counts); precision loss is not reachable"
            )]
            let gap_cost = col as f64 * GAP_PENALTY;
            cost[0][col] = gap_cost;
            step[0][col] = Step::PredictedGap;
        }

        for (row, &gold_id) in gold.iter().enumerate() {
            let row = row + 1;
            let gold_dist = Dist::from(&gold_structure.nodes[gold_id].stage);
            for (col, &predicted_id) in predicted.iter().enumerate() {
                let col = col + 1;
                let predicted_dist = Dist::from(&predicted_structure.nodes[predicted_id].stage);
                let substitution_cost = 1.0 - histogram_intersection(&gold_dist, &predicted_dist);

                let diagonal = cost[row - 1][col - 1] + substitution_cost;
                let gold_gap = cost[row - 1][col] + GAP_PENALTY;
                let predicted_gap = cost[row][col - 1] + GAP_PENALTY;

                let (best_cost, best_step) = if diagonal <= gold_gap && diagonal <= predicted_gap {
                    (diagonal, Step::Diagonal)
                } else if gold_gap <= predicted_gap {
                    (gold_gap, Step::GoldGap)
                } else {
                    (predicted_gap, Step::PredictedGap)
                };
                cost[row][col] = best_cost;
                step[row][col] = best_step;
            }
        }

        Self::backtrack(gold, predicted, &step)
    }

    /// Recover the alignment path from the completed `step` table, walking from
    /// `(gold.len(), predicted.len())` back to `(0, 0)`.
    fn backtrack(
        gold: &[NarrativeNodeId],
        predicted: &[NarrativeNodeId],
        step: &[Vec<Step>],
    ) -> Alignment {
        let mut matches = BTreeMap::new();
        let mut unmatched_gold = BTreeSet::new();
        let mut unmatched_predicted = BTreeSet::new();

        let mut row = gold.len();
        let mut col = predicted.len();
        while row > 0 || col > 0 {
            match step[row][col] {
                Step::Diagonal => {
                    row -= 1;
                    col -= 1;
                    matches.insert(gold[row], predicted[col]);
                }
                Step::GoldGap => {
                    row -= 1;
                    unmatched_gold.insert(gold[row]);
                }
                Step::PredictedGap => {
                    col -= 1;
                    unmatched_predicted.insert(predicted[col]);
                }
            }
        }

        Alignment {
            matches,
            unmatched_gold,
            unmatched_predicted,
        }
    }

    /// Compute just the [`Alignment`], without the aggregate axis/structural
    /// scores [`score`](Scorer::score) builds on top of it.
    #[must_use]
    pub fn align_worlds(&self, gold: &World, predicted: &World) -> Alignment {
        Self::align(
            &gold.story.structure.spine(),
            &predicted.story.structure.spine(),
            &gold.story.structure,
            &predicted.story.structure,
        )
    }
}

/// `precision = matched / predicted_count`; `0.0` (not `NaN`) when
/// `predicted_count == 0` — an empty prediction has no correct guesses to claim
/// credit for.
fn structural_precision(matched: usize, predicted_count: usize) -> f64 {
    if predicted_count == 0 {
        return 0.0;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "node counts are small (bounded by fixture size, far below f64's exact-integer limit)"
    )]
    let (matched, predicted_count) = (matched as f64, predicted_count as f64);
    matched / predicted_count
}

/// `recall = matched / gold_count`; `0.0` (not `NaN`) when `gold_count == 0`
/// (unreachable for a validated fixture — `NarrativeStructure::validate`
/// requires a non-empty node set — but handled defensively rather than
/// panicking).
fn structural_recall(matched: usize, gold_count: usize) -> f64 {
    if gold_count == 0 {
        return 0.0;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "node counts are small (bounded by fixture size, far below f64's exact-integer limit)"
    )]
    let (matched, gold_count) = (matched as f64, gold_count as f64);
    matched / gold_count
}

/// The harmonic mean of `precision` and `recall`; `0.0` (not `NaN` from `0/0`)
/// when both are `0.0`.
fn harmonic_mean(precision: f64, recall: f64) -> f64 {
    if precision <= 0.0 && recall <= 0.0 {
        return 0.0;
    }
    2.0 * precision * recall / (precision + recall)
}

/// Average a non-empty slice of [`DistScore`]s field-wise.
///
/// Returns `None` for an empty slice, so the caller can omit the axis entirely
/// rather than inserting a garbage zero/NaN score for an axis no matched pair
/// ever populated.
fn average_dist_scores(scores: &[DistScore]) -> Option<DistScore> {
    if scores.is_empty() {
        return None;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "the number of matched node pairs is small (bounded by fixture size)"
    )]
    let count = scores.len() as f64;
    let zero = DistScore {
        histogram_intersection: 0.0,
        cross_entropy: 0.0,
        top1_accuracy: 0.0,
        kendall_tau: 0.0,
    };
    let sum = scores.iter().fold(zero, |accumulator, score| DistScore {
        histogram_intersection: accumulator.histogram_intersection + score.histogram_intersection,
        cross_entropy: accumulator.cross_entropy + score.cross_entropy,
        top1_accuracy: accumulator.top1_accuracy + score.top1_accuracy,
        kendall_tau: accumulator.kendall_tau + score.kendall_tau,
    });
    Some(DistScore {
        histogram_intersection: sum.histogram_intersection / count,
        cross_entropy: sum.cross_entropy / count,
        top1_accuracy: sum.top1_accuracy / count,
        kendall_tau: sum.kendall_tau / count,
    })
}

impl Scorer for AlignmentScorer {
    fn score(&self, gold: &World, predicted: &World) -> Report {
        let alignment = self.align_worlds(gold, predicted);

        let matched = alignment.matches.len();
        let gold_count = gold.story.structure.spine().len();
        let predicted_count = predicted.story.structure.spine().len();
        let structural_precision = structural_precision(matched, predicted_count);
        let structural_recall = structural_recall(matched, gold_count);
        let structural_f1 = harmonic_mean(structural_precision, structural_recall);

        let mut stage_scores = Vec::with_capacity(matched);
        let mut functions_scores = Vec::with_capacity(matched);
        let mut motifs_scores = Vec::with_capacity(matched);
        let mut situation_scores = Vec::new();

        for (&gold_id, &predicted_id) in &alignment.matches {
            let gold_node = &gold.story.structure.nodes[gold_id];
            let predicted_node = &predicted.story.structure.nodes[predicted_id];

            stage_scores.push(DistScore::compute(
                &Dist::from(&gold_node.stage),
                &Dist::from(&predicted_node.stage),
            ));
            functions_scores.push(DistScore::compute(
                &Dist::from(&gold_node.functions),
                &Dist::from(&predicted_node.functions),
            ));
            motifs_scores.push(DistScore::compute(
                &Dist::from(&gold_node.motifs),
                &Dist::from(&predicted_node.motifs),
            ));

            // `situation` is `Option<ScoredOne<_>>`: `None` means "this axis ~keep
            // does not apply to this beat", not "the distribution is empty", ~keep
            // so a pair only counts toward the axis average when both sides ~keep
            // scored it — otherwise an irrelevant beat would be scored as if ~keep
            // gold and predicted disagreed. ~keep
            if let (Some(gold_situation), Some(predicted_situation)) =
                (&gold_node.situation, &predicted_node.situation)
            {
                situation_scores.push(DistScore::compute(
                    &Dist::from(gold_situation),
                    &Dist::from(predicted_situation),
                ));
            }
        }

        let mut axes = BTreeMap::new();
        if let Some(score) = average_dist_scores(&stage_scores) {
            axes.insert("stage".to_string(), score);
        }
        if let Some(score) = average_dist_scores(&functions_scores) {
            axes.insert("functions".to_string(), score);
        }
        if let Some(score) = average_dist_scores(&motifs_scores) {
            axes.insert("motifs".to_string(), score);
        }
        if let Some(score) = average_dist_scores(&situation_scores) {
            axes.insert("situation".to_string(), score);
        }

        Report {
            axes,
            alignment,
            structural_precision,
            structural_recall,
            structural_f1,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use monomyth_core::{
        Content, ContentKind, ContentPrompt, EdgeKind, Location, NarrativeEdge, NarrativeNode,
        NarrativeStructure, NodeKind, Player, RngState, SCHEMA_VERSION, ScoredOne, Story, Weight,
        World, WorldMeta, WorldState,
    };
    use monomyth_frameworks::MonomythStage;
    use slotmap::SlotMap;

    use super::*;

    const EPSILON: f64 = 1e-9;

    fn synopsis(hint: &str) -> Content {
        Content::empty(ContentPrompt::new(ContentKind::Synopsis, hint))
    }

    fn choice_label() -> Content {
        Content::empty(ContentPrompt::new(ContentKind::Choice, ""))
    }

    /// Wrap a hand-built, already-valid [`NarrativeStructure`] into a minimal
    /// [`World`] (a bare single room, no entities/items) so `Scorer::score` and
    /// `World::validate` have a complete, well-formed value to work with.
    fn world_from_structure(structure: NarrativeStructure) -> World {
        let mut locations = SlotMap::with_key();
        let room = locations.insert(Location {
            name: Content::empty(ContentPrompt::new(ContentKind::Name, "a bare room")),
            description: Content::empty(ContentPrompt::new(
                ContentKind::Description,
                "a bare room",
            )),
            exits: BTreeMap::new(),
            entities: BTreeSet::new(),
            items: BTreeSet::new(),
        });
        let cursor = structure.root();
        let world = World {
            meta: WorldMeta {
                seed: 0,
                schema_version: SCHEMA_VERSION,
                title: Content::empty(ContentPrompt::new(ContentKind::Title, "a test fixture")),
            },
            locations,
            entities: SlotMap::with_key(),
            items: SlotMap::with_key(),
            player: Player::new(room),
            story: Story {
                structure,
                plot: None,
                quests: SlotMap::with_key(),
            },
            state: WorldState {
                cursor,
                ..WorldState::default()
            },
            rng: RngState::new(0),
        };
        world
            .validate()
            .expect("world_from_structure always builds a valid World");
        world
    }

    /// Build a linear-chain [`World`] from `(label, stage)` pairs, connected
    /// root-to-tail by [`EdgeKind::Sequence`] edges, with the last node the sole
    /// ending.
    fn linear_world(stages: &[(&str, MonomythStage)]) -> World {
        assert!(!stages.is_empty(), "a linear_world fixture needs >=1 node");

        let mut nodes = SlotMap::with_key();
        let mut ids = Vec::with_capacity(stages.len());
        for &(label, stage) in stages {
            let id = nodes.insert(NarrativeNode::new(
                label,
                NodeKind::Beat,
                stage,
                synopsis(label),
            ));
            ids.push(id);
        }
        for window in ids.windows(2) {
            let &[source, target] = window else {
                unreachable!("windows(2) always yields a 2-element slice")
            };
            nodes[source].out.push(NarrativeEdge::new(
                target,
                EdgeKind::Sequence,
                choice_label(),
            ));
        }

        let root = ids[0];
        let tail = *ids.last().expect("non-empty stages");
        let mut structure = NarrativeStructure {
            nodes,
            root,
            endings: BTreeSet::from([tail]),
        };
        structure.recompute_kinds();
        structure
            .validate()
            .expect("linear_world always builds a single-source, acyclic, reconverging DAG");

        world_from_structure(structure)
    }

    /// A built [`World`]'s spine node ids, in spine order.
    fn spine_ids(world: &World) -> Vec<NarrativeNodeId> {
        world.story.structure.spine()
    }


    #[test]
    fn identical_structures_should_align_perfectly() {
        let gold = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("B", MonomythStage::SupernaturalAid),
            ("C", MonomythStage::CrossingTheFirstThreshold),
        ]);
        // Independently built (fresh SlotMap), not cloned, so ids differ. ~keep
        let predicted = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("B", MonomythStage::SupernaturalAid),
            ("C", MonomythStage::CrossingTheFirstThreshold),
        ]);

        let gold_ids = spine_ids(&gold);
        let predicted_ids = spine_ids(&predicted);

        let report = AlignmentScorer.score(&gold, &predicted);

        let expected_matches: BTreeMap<_, _> = gold_ids
            .iter()
            .copied()
            .zip(predicted_ids.iter().copied())
            .collect();
        assert_eq!(report.alignment.matches, expected_matches);
        assert!(report.alignment.unmatched_gold.is_empty());
        assert!(report.alignment.unmatched_predicted.is_empty());

        assert!((report.structural_precision - 1.0).abs() < EPSILON);
        assert!((report.structural_recall - 1.0).abs() < EPSILON);
        assert!((report.structural_f1 - 1.0).abs() < EPSILON);

        let stage_score = report.axes.get("stage").expect("stage axis always present");
        assert!((stage_score.histogram_intersection - 1.0).abs() < EPSILON);
        assert!((stage_score.top1_accuracy - 1.0).abs() < EPSILON);
    }


    #[test]
    fn predicted_missing_a_middle_node_should_leave_it_unmatched_and_align_the_ends() {
        let gold = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("B", MonomythStage::SupernaturalAid),
            ("C", MonomythStage::CrossingTheFirstThreshold),
        ]);
        let predicted = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("C", MonomythStage::CrossingTheFirstThreshold),
        ]);

        let gold_ids = spine_ids(&gold);
        let predicted_ids = spine_ids(&predicted);

        let report = AlignmentScorer.score(&gold, &predicted);

        let mut expected_matches = BTreeMap::new();
        expected_matches.insert(gold_ids[0], predicted_ids[0]);
        expected_matches.insert(gold_ids[2], predicted_ids[1]);
        assert_eq!(report.alignment.matches, expected_matches);
        assert_eq!(
            report.alignment.unmatched_gold,
            BTreeSet::from([gold_ids[1]])
        );
        assert!(report.alignment.unmatched_predicted.is_empty());

        assert!((report.structural_precision - 1.0).abs() < EPSILON);
        assert!((report.structural_recall - (2.0 / 3.0)).abs() < EPSILON);
        let expected_f1 = 2.0 * 1.0 * (2.0 / 3.0) / (1.0 + 2.0 / 3.0);
        assert!((report.structural_f1 - expected_f1).abs() < EPSILON);
    }


    #[test]
    fn predicted_extra_node_should_leave_it_unmatched_and_reduce_precision() {
        let gold = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("B", MonomythStage::SupernaturalAid),
        ]);
        let predicted = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("X", MonomythStage::TheRoadOfTrials),
            ("B", MonomythStage::SupernaturalAid),
        ]);

        let gold_ids = spine_ids(&gold);
        let predicted_ids = spine_ids(&predicted);

        let report = AlignmentScorer.score(&gold, &predicted);

        let mut expected_matches = BTreeMap::new();
        expected_matches.insert(gold_ids[0], predicted_ids[0]);
        expected_matches.insert(gold_ids[1], predicted_ids[2]);
        assert_eq!(report.alignment.matches, expected_matches);
        assert!(report.alignment.unmatched_gold.is_empty());
        assert_eq!(
            report.alignment.unmatched_predicted,
            BTreeSet::from([predicted_ids[1]])
        );

        assert!((report.structural_recall - 1.0).abs() < EPSILON);
        assert!((report.structural_precision - (2.0 / 3.0)).abs() < EPSILON);
    }


    /// gold = [A(CallToAdventure), B(SupernaturalAid), C(CrossingTheFirstThreshold)],
    /// predicted = [A(CallToAdventure), B(TheRoadOfTrials), C(CrossingTheFirstThreshold)]
    /// — only B's stage is wrong, and `TheRoadOfTrials`/`SupernaturalAid` share no
    /// support (both are primary-only `ScoredOne`s), so `histogram_intersection`
    /// for that pair is exactly `0.0` and its substitution cost is exactly `1.0`
    /// — the same as `GAP_PENALTY`.
    ///
    /// Hand-computed total path costs for the two candidate alignments of the
    /// middle position:
    /// - "substitute B here" (diagonal through all three positions): cost is
    ///   `0(A/A) + 1.0(B/B mismatch) + 0(C/C)` = `1.0`.
    /// - "gap gold B and gap predicted B" (skip both middle nodes instead of
    ///   matching them): cost is
    ///   `0(A/A) + GAP_PENALTY(gold B) + GAP_PENALTY(predicted B) + 0(C/C)` = `2.0`.
    ///
    /// `1.0 < 2.0`, so the DP strictly prefers substituting through the bad match
    /// over opening two gaps — B still aligns to B despite the wrong label.
    #[test]
    fn relabeled_stage_should_still_align_despite_a_bad_substitution() {
        let gold = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("B", MonomythStage::SupernaturalAid),
            ("C", MonomythStage::CrossingTheFirstThreshold),
        ]);
        let predicted = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("B", MonomythStage::TheRoadOfTrials),
            ("C", MonomythStage::CrossingTheFirstThreshold),
        ]);

        let gold_ids = spine_ids(&gold);
        let predicted_ids = spine_ids(&predicted);

        let report = AlignmentScorer.score(&gold, &predicted);

        let expected_matches: BTreeMap<_, _> = gold_ids
            .iter()
            .copied()
            .zip(predicted_ids.iter().copied())
            .collect();
        assert_eq!(
            report.alignment.matches, expected_matches,
            "B must still align to B, not be left unmatched, despite the stage mismatch"
        );
        assert!(report.alignment.unmatched_gold.is_empty());
        assert!(report.alignment.unmatched_predicted.is_empty());

        // Perfect structural alignment (every node matched); the classification ~keep
        // failure shows up in the stage axis, not in P/R/F1. ~keep
        assert!((report.structural_f1 - 1.0).abs() < EPSILON);

        let stage_score = report.axes.get("stage").expect("stage axis always present");
        assert!((stage_score.histogram_intersection - (2.0 / 3.0)).abs() < EPSILON);
    }


    /// gold = [A, B, C], predicted = [B, A, C] (A and B's stages are swapped in
    /// position). A monotonic aligner cannot un-swap adjacent nodes: matching
    /// gold-A to predicted-A and gold-B to predicted-B would require a
    /// non-monotonic correspondence, which Needleman-Wunsch's strictly
    /// increasing row/column indices forbid by construction.
    ///
    /// Stages here are disjoint point masses, so any mismatched pair costs
    /// exactly `1.0` and any matched pair costs exactly `0.0`. Two candidate
    /// alignments of the `[A, B]` vs. `[B, A]` prefix both cost `2.0`: two
    /// straight-through mismatched substitutions (`1.0 + 1.0`), or gapping one
    /// node on each side (`GAP_PENALTY + GAP_PENALTY = 1.0 + 1.0`) — a genuine
    /// tie. The documented tie-break (diagonal preferred over any gap) takes
    /// the straight-through path: gold-A matches predicted-B, gold-B matches
    /// predicted-A, gold-C matches predicted-C.
    ///
    /// This is an acceptable, not merely tolerated, result: it costs exactly as
    /// much as gapping would, keeps every node matched (so the count-based
    /// P/R/F1 stays perfect, 3/3), and the swap's actual damage shows up
    /// honestly in the stage axis's histogram-intersection average — exactly
    /// the right signal for "structural order diverged, but node count
    /// matched," rather than an arbitrarily-chosen silent gap.
    #[test]
    fn adjacent_swap_should_align_straight_through_by_the_diagonal_tie_break() {
        let gold = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("B", MonomythStage::SupernaturalAid),
            ("C", MonomythStage::CrossingTheFirstThreshold),
        ]);
        let predicted = linear_world(&[
            ("B", MonomythStage::SupernaturalAid),
            ("A", MonomythStage::CallToAdventure),
            ("C", MonomythStage::CrossingTheFirstThreshold),
        ]);

        let gold_ids = spine_ids(&gold);
        let predicted_ids = spine_ids(&predicted);

        let report = AlignmentScorer.score(&gold, &predicted);

        let mut expected_matches = BTreeMap::new();
        expected_matches.insert(gold_ids[0], predicted_ids[0]); // gold-A <-> pred-B ~keep
        expected_matches.insert(gold_ids[1], predicted_ids[1]); // gold-B <-> pred-A ~keep
        expected_matches.insert(gold_ids[2], predicted_ids[2]); // gold-C <-> pred-C ~keep
        assert_eq!(report.alignment.matches, expected_matches);
        assert!(report.alignment.unmatched_gold.is_empty());
        assert!(report.alignment.unmatched_predicted.is_empty());

        assert!((report.structural_f1 - 1.0).abs() < EPSILON);

        // Average histogram_intersection: (0.0 + 0.0 + 1.0) / 3 = 1/3 — the ~keep
        // swap's damage is visible here, not in the structural counts. ~keep
        let stage_score = report.axes.get("stage").expect("stage axis always present");
        assert!((stage_score.histogram_intersection - (1.0 / 3.0)).abs() < EPSILON);
    }


    /// gold node: `ScoredOne { primary: TheMeetingWithTheGoddess, alternatives:
    /// {WomanAsTemptress: 350} }` vs a predicted node that scores
    /// `WomanAsTemptress` as its sole primary (no alternatives).
    ///
    /// gold `Dist` = `{TheMeetingWithTheGoddess: 1000/1350, WomanAsTemptress:
    /// 350/1350}` = `{0.740740..., 0.259259...}` (primary carries the implicit
    /// `Weight::FULL` = 1000; see `Dist::from(&ScoredOne)`'s doc).
    /// predicted `Dist` = `{WomanAsTemptress: 1.0}`.
    ///
    /// `histogram_intersection = min(0.740740.., 0.0) + min(0.259259.., 1.0)
    /// = 0.0 + 0.259259... = 0.259259...` (`259/1000` after permille
    /// quantization; `350/1350 = 0.259259259...`).
    #[test]
    fn scored_split_should_score_the_hand_computed_partial_overlap() {
        let mut gold_stage = ScoredOne::new(MonomythStage::TheMeetingWithTheGoddess);
        gold_stage.insert_alternative(MonomythStage::WomanAsTemptress, Weight::new(350));

        let mut gold_nodes = SlotMap::with_key();
        let gold_root = gold_nodes.insert({
            let mut node = NarrativeNode::new(
                "GoddessAndTemptress",
                NodeKind::Ending,
                MonomythStage::TheMeetingWithTheGoddess,
                synopsis("split"),
            );
            node.stage = gold_stage;
            node
        });
        let gold_structure = NarrativeStructure {
            nodes: gold_nodes,
            root: gold_root,
            endings: BTreeSet::from([gold_root]),
        };

        let mut predicted_nodes = SlotMap::with_key();
        let predicted_root = predicted_nodes.insert(NarrativeNode::new(
            "GoddessAndTemptress",
            NodeKind::Ending,
            MonomythStage::WomanAsTemptress,
            synopsis("split"),
        ));
        let predicted_structure = NarrativeStructure {
            nodes: predicted_nodes,
            root: predicted_root,
            endings: BTreeSet::from([predicted_root]),
        };

        let gold = world_from_structure(gold_structure);
        let predicted = world_from_structure(predicted_structure);

        let report = AlignmentScorer.score(&gold, &predicted);

        assert_eq!(report.alignment.matches.len(), 1);
        let stage_score = report.axes.get("stage").expect("stage axis always present");
        let expected = 350.0 / 1350.0;
        assert!(
            (stage_score.histogram_intersection - expected).abs() < EPSILON,
            "expected {expected}, got {}",
            stage_score.histogram_intersection
        );
    }


    /// `NarrativeStructure::spine` always yields at least the root (it pushes
    /// `current` before checking for an out-edge), and `NarrativeStructure::validate`
    /// requires a non-empty `endings` set, so a zero-node spine is not
    /// constructible for any valid `World`. This test instead represents "the
    /// extractor found (almost) nothing" honestly as a 1-node predicted spine
    /// against a 3-node gold spine — the closest reachable analogue of "empty
    /// prediction" for a validated fixture.
    #[test]
    fn near_empty_predicted_spine_should_not_panic_and_should_reflect_low_recall() {
        let gold = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("B", MonomythStage::SupernaturalAid),
            ("C", MonomythStage::CrossingTheFirstThreshold),
        ]);
        let predicted = linear_world(&[("Z", MonomythStage::Apotheosis)]);

        let gold_ids = spine_ids(&gold);
        let predicted_ids = spine_ids(&predicted);
        assert_eq!(
            predicted_ids.len(),
            1,
            "the degenerate predicted spine is 1 node"
        );

        // Reaching this assertion at all confirms `score` does not panic on the ~keep
        // degenerate 1-node-vs-3-node case. ~keep
        let report = AlignmentScorer.score(&gold, &predicted);

        // `Apotheosis` shares no support with any gold stage here, so every ~keep
        // substitution costs 1.0, tying every gap-only path (`cost[i][1] = ~keep
        // i * GAP_PENALTY` either way). Backtracking starts at `(3, 1)`, where ~keep
        // the diagonal tie-break applies first: gold-C (the last gold node) ~keep
        // aligns to predicted-Z, and the walk then consumes the remaining gold ~keep
        // nodes (B, then A) as gaps before reaching `(0, 0)`. ~keep
        assert_eq!(report.alignment.matches.len(), 1);
        assert_eq!(report.alignment.matches[&gold_ids[2]], predicted_ids[0]);
        assert_eq!(
            report.alignment.unmatched_gold,
            BTreeSet::from([gold_ids[0], gold_ids[1]])
        );
        assert!(report.alignment.unmatched_predicted.is_empty());

        assert!((report.structural_recall - (1.0 / 3.0)).abs() < EPSILON);
        assert!((report.structural_precision - 1.0).abs() < EPSILON);
        assert!(report.structural_f1.is_finite(), "f1 must never be NaN");
        assert!(!report.structural_f1.is_nan(), "f1 must never be NaN");
    }


    #[test]
    fn alignment_scorer_report_fingerprint_should_be_deterministic() {
        let gold = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("B", MonomythStage::SupernaturalAid),
        ]);
        let predicted = linear_world(&[
            ("A", MonomythStage::CallToAdventure),
            ("B", MonomythStage::SupernaturalAid),
        ]);

        let report_one = AlignmentScorer.score(&gold, &predicted);
        let report_two = AlignmentScorer.score(&gold, &predicted);
        assert_eq!(
            crate::report::report_fingerprint(&report_one),
            crate::report::report_fingerprint(&report_two)
        );
    }
}
