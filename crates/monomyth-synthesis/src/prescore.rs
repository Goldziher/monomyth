//! A cheap, pure, deterministic pre-judge assessment of a candidate.
//!
//! The LLM judge ([`crate::judge`]) is the authority on candidate quality, but
//! it is a black box: a reviewer reading a `.context.json` sees a score with no
//! independent check on it. [`pre_score`] adds a deterministic second opinion —
//! computed with no LLM call, from the candidate, its grounding, and the
//! framework stages the run targeted — that a reviewer can hold up against the
//! judge's number ("the judge said 82, but deterministic phase-coverage was only
//! 6/17 — look harder").
//!
//! It is **advisory only**: recorded in the review trail, never mixed into the
//! judge's [`weighted_score`](crate::weighted_score) and never used to accept or
//! reject a candidate. Gating on a hand-tuned heuristic would either ship an
//! under-collected law or, on a low score, discard the `missing_phases` the loop
//! needs to recover. Shipping the signal first leaves the door open for a later
//! task to promote it to a gate once its thresholds are empirically calibrated.
//!
//! Determinism: every sub-score is a ratio of integer counts over
//! ordered/`BTreeSet` iteration, so the result is bit-stable across runs and
//! platforms — it reads no wall clock and no RNG.

use std::collections::BTreeSet;

use monomyth_knowledge::Passage;
use serde::Serialize;

use crate::antileak::normalize_words;
use crate::candidate::CandidateLaw;

/// Minimum length (in characters) for a normalized word to count as a *content*
/// token, dropping short function words ("of", "the", "a") that would otherwise
/// inflate overlap between unrelated texts.
const MIN_TOKEN_LEN: usize = 3;

/// Content tokens a candidate item and a framework stage must share for the item
/// to count that stage as covered. One is deliberately permissive: coverage is
/// an advisory recall signal, not a precise match.
const MIN_STAGE_OVERLAP_TOKENS: usize = 1;

/// Content tokens an item description and a grounding passage must share for the
/// item to count as grounded. Two, so a single incidental shared word does not
/// mark an item grounded.
const MIN_GROUNDING_OVERLAP_TOKENS: usize = 2;

/// Weight of [`PreScore::phase_coverage`] in [`PreScore::overall`]. Coverage
/// dominates because under-collection is the documented primary failure mode
/// (see `judge::DEFAULT_CRITERIA`).
const COVERAGE_WEIGHT: f64 = 0.5;
/// Weight of [`PreScore::ordering_monotonicity`] in [`PreScore::overall`].
const ORDERING_WEIGHT: f64 = 0.2;
/// Weight of [`PreScore::grounding_overlap`] in [`PreScore::overall`].
const GROUNDING_WEIGHT: f64 = 0.3;

/// A deterministic, no-LLM assessment of a candidate law. All fields are in
/// `0.0..=1.0`.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct PreScore {
    /// Fraction of the target framework stages that appear covered by some
    /// candidate item, by content-token overlap.
    pub phase_coverage: f64,
    /// Fraction of adjacent covered-stage pairs whose covering candidate items
    /// appear in non-decreasing order — a proxy for "the candidate follows the
    /// framework's narrative order". `1.0` when fewer than two stages are
    /// covered (no ordering to violate).
    pub ordering_monotonicity: f64,
    /// Fraction of candidate items whose description shares content with at
    /// least one grounding passage — a proxy for "nothing was invented".
    pub grounding_overlap: f64,
    /// A single reviewer-facing blend of the three signals (weights
    /// [`COVERAGE_WEIGHT`], [`ORDERING_WEIGHT`], [`GROUNDING_WEIGHT`]).
    pub overall: f64,
}

/// The `0.0..=1.0` ratio `numerator / denominator`, or `0.0` when the
/// denominator is zero (an empty population scores zero, never a NaN).
// The counts are item/stage/passage cardinalities — at most a few dozen, far
// below f64's 2^52 exact-integer range — so the usize->f64 cast is exact here.
#[allow(clippy::cast_precision_loss)]
fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

/// The set of content tokens in `text`: normalized as the anti-leak gate
/// normalizes ([`normalize_words`]), then restricted to words of at least
/// [`MIN_TOKEN_LEN`] characters. A `BTreeSet` so overlap counts are
/// order-independent and deterministic.
fn content_tokens(text: &str) -> BTreeSet<String> {
    normalize_words(text)
        .into_iter()
        .filter(|word| word.chars().count() >= MIN_TOKEN_LEN)
        .collect()
}

/// The number of content tokens shared between two token sets.
fn shared(left: &BTreeSet<String>, right: &BTreeSet<String>) -> usize {
    left.intersection(right).count()
}

/// Compute a deterministic pre-score for `candidate` against its `grounding`
/// and the `framework_stages` the run targeted (e.g. the Campbell stage names).
///
/// Pure: no I/O, no wall clock, no RNG. Intended to be called on each iteration
/// alongside the LLM judge, purely to record an advisory signal.
#[must_use]
pub fn pre_score(
    candidate: &CandidateLaw,
    grounding: &[Passage],
    framework_stages: &[&str],
) -> PreScore {
    let item_tokens: Vec<BTreeSet<String>> = candidate
        .items
        .iter()
        .map(|item| {
            let mut tokens = content_tokens(&item.name);
            tokens.extend(content_tokens(&item.description));
            tokens
        })
        .collect();

    // Phase coverage + the first item index covering each covered stage, in
    // framework order (for the ordering check below).
    let mut first_cover_indices: Vec<usize> = Vec::new();
    for stage in framework_stages {
        let stage_tokens = content_tokens(stage);
        let covering = item_tokens
            .iter()
            .position(|tokens| shared(&stage_tokens, tokens) >= MIN_STAGE_OVERLAP_TOKENS);
        if let Some(index) = covering {
            first_cover_indices.push(index);
        }
    }
    let phase_coverage = ratio(first_cover_indices.len(), framework_stages.len());

    let ordering_monotonicity = if first_cover_indices.len() < 2 {
        1.0
    } else {
        let good = first_cover_indices
            .windows(2)
            .filter(|pair| pair[1] >= pair[0])
            .count();
        ratio(good, first_cover_indices.len() - 1)
    };

    let passage_tokens: Vec<BTreeSet<String>> = grounding
        .iter()
        .map(|passage| content_tokens(&passage.text))
        .collect();
    let grounded_items = candidate
        .items
        .iter()
        .filter(|item| {
            let description = content_tokens(&item.description);
            passage_tokens
                .iter()
                .any(|passage| shared(&description, passage) >= MIN_GROUNDING_OVERLAP_TOKENS)
        })
        .count();
    let grounding_overlap = ratio(grounded_items, candidate.items.len());

    let overall = COVERAGE_WEIGHT * phase_coverage
        + ORDERING_WEIGHT * ordering_monotonicity
        + GROUNDING_WEIGHT * grounding_overlap;

    PreScore {
        phase_coverage,
        ordering_monotonicity,
        grounding_overlap,
        overall,
    }
}

#[cfg(test)]
mod tests {
    use monomyth_knowledge::Namespace;

    use super::*;
    use crate::candidate::CandidateItem;

    /// Assert two scores are equal within a tight tolerance. The pre-score is a
    /// ratio of small integer counts, so the values are exact in practice; this
    /// only exists to satisfy clippy's `float_cmp` without weakening intent.
    fn approx(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < 1e-9,
            "expected {expected}, got {actual}"
        );
    }

    fn item(name: &str, description: &str) -> CandidateItem {
        CandidateItem {
            name: name.to_owned(),
            description: description.to_owned(),
            derivable: true,
        }
    }

    fn passage(text: &str) -> Passage {
        Passage {
            text: text.to_owned(),
            source_id: "src".to_owned(),
            score: 1.0,
            namespace: Namespace::Reference,
            license: "in-copyright".to_owned(),
            url: None,
            checksum: None,
            retrieved: None,
        }
    }

    #[test]
    fn full_coverage_scores_one_partial_scores_the_exact_ratio() {
        let candidate = CandidateLaw {
            title: "Arc".to_owned(),
            items: vec![
                item("Departure", "The hero departs the ordinary world."),
                item("Trials", "The hero endures escalating trials."),
            ],
        };
        let full = pre_score(&candidate, &[], &["Departure", "Trials"]);
        approx(full.phase_coverage, 1.0);

        let partial = pre_score(&candidate, &[], &["Departure", "Trials", "Return"]);
        approx(partial.phase_coverage, 2.0 / 3.0);
    }

    #[test]
    fn out_of_order_items_lower_the_ordering_score() {
        // Stage order is [Departure, Trials]; items place "Trials" first, so the
        // covering indices are [1, 0] — one adjacent pair, not monotonic.
        let candidate = CandidateLaw {
            title: "Arc".to_owned(),
            items: vec![
                item("Trials", "The hero endures escalating trials."),
                item("Departure", "The hero departs the ordinary world."),
            ],
        };
        let scored = pre_score(&candidate, &[], &["Departure", "Trials"]);
        approx(scored.ordering_monotonicity, 0.0);
    }

    #[test]
    fn grounding_overlap_counts_only_items_sharing_content_with_a_passage() {
        let candidate = CandidateLaw {
            title: "Arc".to_owned(),
            items: vec![
                item("Grounded", "The hero endures escalating trials."),
                item(
                    "Invented",
                    "Quarks entangle across bureaucratic spreadsheets.",
                ),
            ],
        };
        let grounding = vec![passage(
            "A tale of the hero who endures escalating trials abroad.",
        )];
        let scored = pre_score(&candidate, &grounding, &[]);
        approx(scored.grounding_overlap, 0.5);
    }

    #[test]
    fn is_deterministic_across_repeated_calls() {
        let candidate = CandidateLaw {
            title: "Arc".to_owned(),
            items: vec![
                item("Departure", "The hero departs the ordinary world."),
                item(
                    "Return",
                    "The hero returns transformed and restores the world.",
                ),
            ],
        };
        let grounding = vec![passage(
            "The hero departs, then returns transformed to restore order.",
        )];
        let stages = ["Departure", "Return"];
        let first = pre_score(&candidate, &grounding, &stages);
        let second = pre_score(&candidate, &grounding, &stages);
        assert_eq!(first, second);
    }

    #[test]
    fn empty_inputs_do_not_panic_and_score_zero_coverage() {
        let empty = CandidateLaw {
            title: String::new(),
            items: vec![],
        };
        let scored = pre_score(&empty, &[], &[]);
        approx(scored.phase_coverage, 0.0);
        approx(scored.grounding_overlap, 0.0);
        // No stages to violate ordering over, so ordering is the neutral 1.0.
        approx(scored.ordering_monotonicity, 1.0);
    }
}
