//! [`Scorer`], [`Report`], and the FNV-golden fingerprint over a quantized report.
//!
//! This module defines the scoring seam ADR-0023 calls for.
//! [`crate::alignment::AlignmentScorer`] is the Phase B3 implementation: it
//! matches extracted narrative nodes to gold nodes and fills in [`Alignment`] and
//! [`Report`]'s structural precision/recall/F1 and per-axis [`DistScore`]s. What
//! lives here is the shape a scorer fills in — the aggregate [`Report`], the
//! node [`Alignment`], and a reproducible [`report_fingerprint`] so a `Report`
//! can be pinned the same way generator output and fixtures are.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Serialize, Serializer};

use crate::metrics::DistScore;
use crate::util::fnv1a;
use monomyth_core::{NarrativeNodeId, World};

/// A gold-to-predicted narrative node correspondence, produced by a
/// node-alignment [`Scorer`] (Phase B3's [`crate::alignment::AlignmentScorer`]).
///
/// `matches` pairs every gold node the aligner was able to match to a predicted
/// node, regardless of how good the match's substitution score was — the
/// alignment itself is robust to a bad label; only a true gap (no counterpart on
/// the other side) lands in `unmatched_gold`/`unmatched_predicted`. This is the
/// sharpest-risk piece of the eval harness — a wrong alignment silently corrupts
/// every downstream metric — so it is unit-tested against hand-crafted near-miss
/// DAGs before any aggregate score built on top of it is trusted (ADR-0023's
/// Decision Outcome).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Alignment {
    /// Gold node id -> matched predicted node id, for every node the alignment
    /// scorer was able to match.
    ///
    /// Serialized as a JSON array of `[gold, predicted]` pairs, not an object:
    /// [`NarrativeNodeId`] is a `slotmap` generational key that serializes as a
    /// structured value, and `serde_json` rejects a non-string object key —
    /// serializing the map directly panics any JSON emitter (the `eval` CLI's
    /// report output). Iterating the source `BTreeMap` keeps the canonical,
    /// snapshot-stable order used everywhere else, and matches the shape
    /// [`QuantizedAlignment`] already uses for the fingerprint.
    #[serde(serialize_with = "serialize_node_id_matches")]
    pub matches: BTreeMap<NarrativeNodeId, NarrativeNodeId>,
    /// Gold nodes with no predicted counterpart (a deletion in the alignment).
    pub unmatched_gold: BTreeSet<NarrativeNodeId>,
    /// Predicted nodes with no gold counterpart (an insertion in the alignment).
    pub unmatched_predicted: BTreeSet<NarrativeNodeId>,
}

/// Serialize a [`NarrativeNodeId`]-keyed match map as a JSON array of
/// `[gold, predicted]` pairs, since `serde_json` cannot use a non-string object
/// key. See [`Alignment::matches`].
fn serialize_node_id_matches<S>(
    matches: &BTreeMap<NarrativeNodeId, NarrativeNodeId>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.collect_seq(matches.iter().map(|(&gold, &predicted)| (gold, predicted)))
}

/// The aggregate result of scoring a predicted [`World`] against a gold one.
///
/// Per-axis scores are keyed by a caller-chosen axis name (e.g. `"stage"`,
/// `"functions"`) rather than a fixed field per framework system, so a
/// [`Scorer`] implementation can score whichever axes it covers without this
/// type growing a field per framework enum. `BTreeMap`-keyed for canonical,
/// snapshot-stable iteration order, matching the project-wide convention. Only
/// axes with at least one eligible matched pair are present (see
/// [`crate::alignment::AlignmentScorer::score`]'s per-axis eligibility rules —
/// in particular, `situation` is only eligible when both sides of a matched
/// pair scored one).
///
/// Prose (the `Content` slots) is deliberately not scored anywhere in this
/// type: every fixture's `Content` is `Content::Empty` (ADR-0023), so there is
/// nothing to compare a predicted prose value against yet. A future prose-scoring
/// axis is out of scope until fixtures carry real content.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Report {
    /// Per-axis distribution scores, keyed by axis name, averaged over every
    /// matched `(gold, predicted)` node pair for which the axis applies.
    pub axes: BTreeMap<String, DistScore>,
    /// The gold-to-predicted node correspondence.
    pub alignment: Alignment,
    /// `matched_count / predicted_node_count`; `0.0` (not `NaN`) when there are
    /// no predicted nodes to claim credit for.
    pub structural_precision: f64,
    /// `matched_count / gold_node_count`; `0.0` (not `NaN`) when there are no
    /// gold nodes to have matched (unreachable for a validated fixture, but
    /// handled defensively rather than panicking).
    pub structural_recall: f64,
    /// The harmonic mean of [`structural_precision`](Self::structural_precision)
    /// and [`structural_recall`](Self::structural_recall); `0.0` (not `NaN`)
    /// when both are `0.0`.
    pub structural_f1: f64,
}

/// Scores a predicted [`World`] against a gold [`World`], producing a [`Report`].
///
/// [`crate::alignment::AlignmentScorer`] is the Phase B3 implementation.
pub trait Scorer {
    /// Compare `predicted` against `gold` and produce a [`Report`].
    fn score(&self, gold: &World, predicted: &World) -> Report;
}

/// Quantization step for [`report_fingerprint`]: scores are rounded to the
/// nearest permille (three decimal digits) before hashing.
///
/// Named so the magic `1000.0` does not appear unexplained at the call site;
/// matches the permille scale [`Weight`](monomyth_core::Weight) already uses
/// elsewhere in the contract, for a consistent precision budget across the
/// codebase.
const QUANTIZATION_SCALE: f64 = 1000.0;

/// Round `value` to the nearest permille (three decimal digits) and return it as
/// an integer.
///
/// # Determinism note
///
/// `Report`'s scores are `f64`, and hashing raw `f64` bit patterns risks
/// cross-platform drift: two runs that agree to any reasonable tolerance (e.g.
/// `0.699999999998` vs `0.7`) can still hash to different values if compared
/// bit-for-bit, and transcendental functions like `ln` (used by
/// [`cross_entropy`](crate::cross_entropy)) are not guaranteed bit-identical
/// across platforms/toolchains by IEEE 754. Quantizing to a fixed integer
/// precision before hashing makes the hashed form robust to that class of noise
/// while still catching genuine score drift larger than one permille.
///
/// The four metrics this crate produces are all bounded to small ranges
/// (roughly `-1000..=1000` for the permille-scaled ratio metrics, and a few
/// tens of thousands at most for [`cross_entropy`](crate::cross_entropy)'s
/// smoothed worst case), so the `f64 -> i64` cast never truncates in practice;
/// the lint is silenced rather than threading a fallible path through a pure
/// quantization helper.
#[allow(
    clippy::cast_possible_truncation,
    reason = "quantized scores are bounded to a small range (see doc); truncation is not reachable"
)]
fn quantize(value: f64) -> i64 {
    (value * QUANTIZATION_SCALE).round() as i64
}

/// A `Report` reduced to the exact integer values [`report_fingerprint`] hashes:
/// every score quantized to permille precision, in the same canonical key order
/// as the source `Report` (`BTreeMap` iteration).
#[derive(Serialize)]
struct QuantizedReport {
    axes: BTreeMap<String, QuantizedDistScore>,
    alignment: QuantizedAlignment,
    structural_precision: i64,
    structural_recall: i64,
    structural_f1: i64,
}

/// [`Alignment`] reshaped for [`serde_json`]: `serde_json` requires object keys
/// to be strings, and [`NarrativeNodeId`] (a `slotmap` generational key)
/// serializes as a structured value, not a string — so `Alignment::matches`
/// (a `BTreeMap<NarrativeNodeId, NarrativeNodeId>`) cannot serialize directly
/// via `serde_json::to_vec`. `Vec<(NarrativeNodeId, NarrativeNodeId)>` and
/// `Vec<NarrativeNodeId>` serialize as plain JSON arrays instead, and iterating
/// the source `BTreeMap`/`BTreeSet`s preserves the same canonical, snapshot-
/// stable order this crate uses everywhere else.
#[derive(Serialize)]
struct QuantizedAlignment {
    matches: Vec<(NarrativeNodeId, NarrativeNodeId)>,
    unmatched_gold: Vec<NarrativeNodeId>,
    unmatched_predicted: Vec<NarrativeNodeId>,
}

impl From<&Alignment> for QuantizedAlignment {
    fn from(alignment: &Alignment) -> Self {
        Self {
            matches: alignment
                .matches
                .iter()
                .map(|(&gold, &predicted)| (gold, predicted))
                .collect(),
            unmatched_gold: alignment.unmatched_gold.iter().copied().collect(),
            unmatched_predicted: alignment.unmatched_predicted.iter().copied().collect(),
        }
    }
}

/// [`DistScore`] with every `f64` field replaced by its quantized `i64`.
#[derive(Serialize)]
struct QuantizedDistScore {
    histogram_intersection: i64,
    cross_entropy: i64,
    top1_accuracy: i64,
    kendall_tau: i64,
}

impl From<&DistScore> for QuantizedDistScore {
    fn from(score: &DistScore) -> Self {
        Self {
            histogram_intersection: quantize(score.histogram_intersection),
            cross_entropy: quantize(score.cross_entropy),
            top1_accuracy: quantize(score.top1_accuracy),
            kendall_tau: quantize(score.kendall_tau),
        }
    }
}

/// Hash a [`Report`] into a stable `u64` fingerprint, quantizing every score to
/// permille precision first.
///
/// This is the `Report` analogue of `monomyth-gen`'s golden-seed FNV hash: it
/// lets a `Report` be pinned in a regression test the same way generator output
/// is, without the hash drifting across platforms on floating-point noise (see
/// [`quantize`]'s doc). Two `Report`s whose scores agree to permille precision
/// fingerprint identically, even if their raw `f64` bit patterns differ.
///
/// # Panics
///
/// Never in practice: [`QuantizedReport`] is built entirely from `String`-keyed
/// `BTreeMap`s and plain integer/struct/array fields, none of which
/// [`serde_json::to_vec`] can fail to serialize. The `.expect` documents that
/// assumption rather than threading a `Result` through a function whose only
/// fallible step is unreachable for this type.
#[must_use]
pub fn report_fingerprint(report: &Report) -> u64 {
    let quantized = QuantizedReport {
        axes: report
            .axes
            .iter()
            .map(|(axis, score)| (axis.clone(), QuantizedDistScore::from(score)))
            .collect(),
        alignment: QuantizedAlignment::from(&report.alignment),
        structural_precision: quantize(report.structural_precision),
        structural_recall: quantize(report.structural_recall),
        structural_f1: quantize(report.structural_f1),
    };
    let bytes = serde_json::to_vec(&quantized)
        .expect("QuantizedReport contains no non-serializable types (no floats, no maps with non-string keys at the top level)");
    fnv1a(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn score(
        histogram_intersection: f64,
        cross_entropy: f64,
        top1_accuracy: f64,
        kendall_tau: f64,
    ) -> DistScore {
        DistScore {
            histogram_intersection,
            cross_entropy,
            top1_accuracy,
            kendall_tau,
        }
    }

    #[test]
    fn report_fingerprint_should_be_deterministic_for_identical_reports() {
        let mut report = Report::default();
        report
            .axes
            .insert("stage".to_string(), score(0.8, 0.693, 1.0, 0.0));

        assert_eq!(report_fingerprint(&report), report_fingerprint(&report));
    }

    #[test]
    fn report_fingerprint_should_treat_sub_permille_differences_as_identical() {
        let mut a = Report::default();
        a.axes
            .insert("stage".to_string(), score(0.800_000_1, 0.693, 1.0, 0.0));

        let mut b = Report::default();
        b.axes
            .insert("stage".to_string(), score(0.799_999_9, 0.693, 1.0, 0.0));

        assert_eq!(
            report_fingerprint(&a),
            report_fingerprint(&b),
            "scores equal after quantization to permille precision must hash identically"
        );
    }

    #[test]
    fn report_fingerprint_should_diverge_on_a_real_score_difference() {
        let mut a = Report::default();
        a.axes
            .insert("stage".to_string(), score(0.8, 0.693, 1.0, 0.0));

        let mut b = Report::default();
        b.axes
            .insert("stage".to_string(), score(0.5, 0.693, 1.0, 0.0));

        assert_ne!(report_fingerprint(&a), report_fingerprint(&b));
    }

    #[test]
    fn report_fingerprint_should_be_sensitive_to_axis_name() {
        let mut a = Report::default();
        a.axes
            .insert("stage".to_string(), score(0.8, 0.693, 1.0, 0.0));

        let mut b = Report::default();
        b.axes
            .insert("functions".to_string(), score(0.8, 0.693, 1.0, 0.0));

        assert_ne!(report_fingerprint(&a), report_fingerprint(&b));
    }

    #[test]
    fn quantize_should_round_to_nearest_permille() {
        assert_eq!(quantize(0.7), 700);
        assert_eq!(quantize(0.699_999_9), 700);
        assert_eq!(quantize(1.0), 1000);
        assert_eq!(quantize(0.0), 0);
    }
}
