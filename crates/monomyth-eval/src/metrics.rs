//! Distribution-comparison metrics: ADR-0023's scored-attribute metric set.
//!
//! Four metrics, each comparing a gold [`Dist`] against a predicted one:
//! [`histogram_intersection`] (the primary metric), [`cross_entropy`] (training
//! alignment), [`top1_accuracy`], and [`kendall_tau`] (rank agreement). All are
//! pure functions of two `Dist<T>` values — no RNG, no IO, no LLM — so a score is
//! exactly reproducible from its inputs, matching the rest of the contract's
//! determinism discipline.

use std::cmp::Ordering;

use serde::Serialize;

use crate::dist::Dist;

/// The smoothing floor [`cross_entropy`] substitutes for a zero predicted
/// probability on a gold-support key.
///
/// Without this floor, a gold key entirely missing from the predicted
/// distribution's support would make `-ln(0)` diverge to `+inf`, which is
/// useless as a regression-testable score (it swamps every other term and is not
/// comparable across runs). `1e-9` is small enough to heavily penalize a missed
/// key (`-ln(1e-9) ~= 20.7` nats, versus `~0` for a well-predicted key) while
/// staying finite and stable under quantization (see [`crate::report`]).
const CROSS_ENTROPY_EPSILON: f64 = 1e-9;

/// Weighted histogram intersection: `sum_k min(gold(k), predicted(k))` over the
/// union of both supports.
///
/// The primary metric of ADR-0023's scored-attribute metric set. `1.0` when the
/// two distributions are identical; `0.0` when they share no support (disjoint).
/// Because both `Dist`s are normalized to sum to `1.0`, the result is always in
/// `0.0..=1.0`.
///
/// ```
/// use monomyth_eval::{Dist, histogram_intersection};
/// use monomyth_core::{ScoredSet, Weight};
/// use monomyth_frameworks::ProppFunction;
///
/// let mut set = ScoredSet::new();
/// set.insert(ProppFunction::Departure, Weight::FULL);
/// let dist = Dist::from(&set);
/// assert_eq!(histogram_intersection(&dist, &dist), 1.0);
/// ```
#[must_use]
pub fn histogram_intersection<T: Ord + Clone>(gold: &Dist<T>, predicted: &Dist<T>) -> f64 {
    union_support(gold, predicted)
        .map(|key| gold.get(key).min(predicted.get(key)))
        .sum()
}

/// Cross-entropy `H(gold, predicted) = -sum_k gold(k) * ln(predicted(k))`, in
/// nats, over the gold distribution's support.
///
/// Lower is better (`0.0` when `predicted` exactly reproduces `gold`'s mass on
/// every gold-support key). A gold-support key absent from `predicted` (implicit
/// probability `0.0`) is smoothed to [`CROSS_ENTROPY_EPSILON`] rather than
/// producing `-ln(0) = +inf`, so the metric stays finite and comparable across
/// runs; see that constant's doc for why `1e-9` was chosen.
///
/// ```
/// use monomyth_eval::{Dist, cross_entropy};
/// use monomyth_core::{ScoredSet, Weight};
/// use monomyth_frameworks::ProppFunction;
///
/// let mut set = ScoredSet::new();
/// set.insert(ProppFunction::Departure, Weight::FULL);
/// let dist = Dist::from(&set);
/// assert!(cross_entropy(&dist, &dist) < 1e-9);
/// ```
#[must_use]
pub fn cross_entropy<T: Ord + Clone>(gold: &Dist<T>, predicted: &Dist<T>) -> f64 {
    gold.iter()
        .map(|(key, gold_probability)| {
            let predicted_probability = predicted.get(key).max(CROSS_ENTROPY_EPSILON);
            -gold_probability * predicted_probability.ln()
        })
        .sum()
}

/// `1.0` iff `gold` and `predicted` agree on their argmax value, else `0.0`.
///
/// Tie-breaking is deterministic: [`Dist::argmax`] resolves a probability tie by
/// canonical `T`-order (the smallest value wins), so this metric never depends on
/// iteration or insertion order. Two empty distributions (`argmax` is `None` on
/// both sides) count as agreement.
///
/// ```
/// use monomyth_eval::{Dist, top1_accuracy};
/// use monomyth_core::{ScoredSet, Weight};
/// use monomyth_frameworks::ProppFunction;
///
/// let mut set = ScoredSet::new();
/// set.insert(ProppFunction::Departure, Weight::FULL);
/// let dist = Dist::from(&set);
/// assert_eq!(top1_accuracy(&dist, &dist), 1.0);
/// ```
#[must_use]
pub fn top1_accuracy<T: Ord + Clone>(gold: &Dist<T>, predicted: &Dist<T>) -> f64 {
    f64::from(u8::from(gold.argmax() == predicted.argmax()))
}

/// Kendall rank correlation coefficient over the *intersecting* support of
/// `gold` and `predicted` (keys with nonzero mass in both), in `-1.0..=1.0`.
///
/// # Definition
///
/// For every unordered pair `{a, b}` drawn from the intersecting support (keys
/// present with nonzero mass in *both* distributions — a key only one side
/// scored carries no rank information on the other side, so it is excluded
/// rather than implicitly ranked last), classify the pair against each
/// distribution's probability order:
///
/// - **concordant** (`+1`): both distributions agree on which of `a`, `b` ranks
///   higher.
/// - **discordant** (`-1`): the distributions disagree.
/// - **tied** (`0`): either distribution scores `a` and `b` equally — probability
///   ties are excluded from both the concordant and discordant counts but still
///   count toward the pair total (this is the conservative "any tie contributes
///   zero" convention, simpler than a tau-b correction and adequate for the
///   small, low-cardinality framework taxonomies this crate scores).
///
/// `tau = (concordant - discordant) / total_pairs`.
///
/// # Edge cases
///
/// - **Empty or singleton intersecting support** (0 or 1 shared keys, so zero
///   pairs — including the fully disjoint case, where the intersection is
///   empty): defined as `1.0` — trivial agreement, since there is no pair to
///   disagree on. This matches the convention that an empty relation is
///   vacuously consistent, and keeps the metric total (never `NaN`) for callers
///   that do not special-case small supports. Note this differs from
///   [`top1_accuracy`], which *does* penalize a disjoint pair of distributions
///   (different argmax) — the two metrics answer different questions ("do the
///   top picks agree" vs. "do the shared candidates rank consistently").
///
/// ```
/// use monomyth_eval::{Dist, kendall_tau};
/// use monomyth_core::{ScoredSet, Weight};
/// use monomyth_frameworks::ProppFunction;
///
/// let mut set = ScoredSet::new();
/// set.insert(ProppFunction::Departure, Weight::FULL);
/// let dist = Dist::from(&set);
/// assert_eq!(kendall_tau(&dist, &dist), 1.0);
/// ```
#[must_use]
pub fn kendall_tau<T: Ord + Clone>(gold: &Dist<T>, predicted: &Dist<T>) -> f64 {
    let shared: Vec<&T> = intersection_support(gold, predicted).collect();
    if shared.len() < 2 {
        return 1.0;
    }

    let mut concordant_minus_discordant: i64 = 0;
    let mut total_pairs: i64 = 0;
    for (index, first) in shared.iter().enumerate() {
        for second in &shared[index + 1..] {
            total_pairs += 1;
            let gold_order = gold.get(first).partial_cmp(&gold.get(second));
            let predicted_order = predicted.get(first).partial_cmp(&predicted.get(second));
            concordant_minus_discordant += pair_contribution(gold_order, predicted_order);
        }
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "pair counts are tiny (bounded by taxonomy size); precision loss is not reachable"
    )]
    let contribution = concordant_minus_discordant as f64;
    #[allow(
        clippy::cast_precision_loss,
        reason = "pair counts are tiny (bounded by taxonomy size); precision loss is not reachable"
    )]
    let pairs = total_pairs as f64;
    contribution / pairs
}

/// `+1` if `gold_order` and `predicted_order` agree (both `Less` or both
/// `Greater`), `-1` if they disagree, `0` if either side is a tie or
/// incomparable (`NaN`, unreachable for the finite probabilities `Dist`
/// produces, but `partial_cmp` still returns `Option`).
fn pair_contribution(gold_order: Option<Ordering>, predicted_order: Option<Ordering>) -> i64 {
    match (gold_order, predicted_order) {
        (Some(Ordering::Less), Some(Ordering::Less))
        | (Some(Ordering::Greater), Some(Ordering::Greater)) => 1,
        (Some(Ordering::Less), Some(Ordering::Greater))
        | (Some(Ordering::Greater), Some(Ordering::Less)) => -1,
        _ => 0,
    }
}

/// The union of two distributions' supports, deduplicated and in canonical
/// `T`-order (both `BTreeMap`-backed, so a merge-style walk would also work, but
/// a sorted-and-deduped collect is simpler and support sizes are small).
///
/// Used by [`histogram_intersection`], where a key present in only one support
/// still contributes (`min` against the other side's implicit `0.0` probability
/// is `0.0`, i.e. a no-op term, so including union-only keys is harmless and
/// keeps the sum over exactly the keys where either distribution has mass).
fn union_support<'a, T: Ord + Clone>(
    a: &'a Dist<T>,
    b: &'a Dist<T>,
) -> impl Iterator<Item = &'a T> {
    let mut keys: Vec<&T> = a.support().chain(b.support()).collect();
    keys.sort_unstable();
    keys.dedup();
    keys.into_iter()
}

/// The intersection of two distributions' supports (keys with nonzero mass in
/// *both*), in canonical `T`-order.
///
/// Used by [`kendall_tau`], where a key only one side scored has no rank
/// relationship to compare — unlike [`union_support`], including it would
/// either require inventing an implicit rank (wrong) or silently skip it inside
/// the pair loop (equivalent but wasteful); computing the true intersection
/// up front is both correct and the simpler loop.
fn intersection_support<'a, T: Ord + Clone>(
    a: &'a Dist<T>,
    b: &'a Dist<T>,
) -> impl Iterator<Item = &'a T> {
    a.support().filter(move |key| b.get(key) > 0.0)
}

/// The four ADR-0023 metrics bundled together for a single attribute comparison.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DistScore {
    /// [`histogram_intersection`] — the primary metric.
    pub histogram_intersection: f64,
    /// [`cross_entropy`], in nats.
    pub cross_entropy: f64,
    /// [`top1_accuracy`].
    pub top1_accuracy: f64,
    /// [`kendall_tau`].
    pub kendall_tau: f64,
}

impl DistScore {
    /// Compute all four metrics for one `(gold, predicted)` pair.
    #[must_use]
    pub fn compute<T: Ord + Clone>(gold: &Dist<T>, predicted: &Dist<T>) -> Self {
        Self {
            histogram_intersection: histogram_intersection(gold, predicted),
            cross_entropy: cross_entropy(gold, predicted),
            top1_accuracy: top1_accuracy(gold, predicted),
            kendall_tau: kendall_tau(gold, predicted),
        }
    }
}

#[cfg(test)]
// Several assertions below compare against 0.0/1.0/-1.0 exactly: these are ~keep
// values the metrics are designed to return exactly (e.g. `top1_accuracy` is ~keep
// `f64::from(u8::from(bool))`, `histogram_intersection`/`kendall_tau` on ~keep
// identical or disjoint inputs reduce to exact sums of exact terms), not ~keep
// values reached by accumulated floating-point arithmetic. Epsilon comparison ~keep
// is still used below wherever the expected value is itself a computed ratio. ~keep
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use monomyth_core::{ScoredSet, Weight};
    use monomyth_frameworks::ProppFunction;

    const EPSILON: f64 = 1e-9;

    fn dist_from(pairs: &[(ProppFunction, u16)]) -> Dist<ProppFunction> {
        let mut set = ScoredSet::new();
        for &(function, weight) in pairs {
            set.insert(function, Weight::new(weight));
        }
        Dist::from(&set)
    }

    #[test]
    fn identical_distributions_should_score_perfectly_on_rank_and_overlap_metrics() {
        let dist = dist_from(&[
            (ProppFunction::Departure, 700),
            (ProppFunction::Struggle, 300),
        ]);

        assert_eq!(histogram_intersection(&dist, &dist), 1.0);
        assert_eq!(top1_accuracy(&dist, &dist), 1.0);
        assert_eq!(kendall_tau(&dist, &dist), 1.0);
    }

    /// Cross-entropy against *itself* is a distribution's own Shannon entropy,
    /// not zero, unless the distribution is a point mass (entropy of a single
    /// certain outcome is `0`). `H(0.7, 0.3) = -(0.7*ln(0.7) + 0.3*ln(0.3))`.
    #[test]
    fn cross_entropy_of_identical_non_degenerate_distributions_equals_self_entropy() {
        let dist = dist_from(&[
            (ProppFunction::Departure, 700),
            (ProppFunction::Struggle, 300),
        ]);
        let expected_entropy = -(0.7 * 0.7_f64.ln() + 0.3 * 0.3_f64.ln());

        assert!((cross_entropy(&dist, &dist) - expected_entropy).abs() < EPSILON);
    }

    #[test]
    fn cross_entropy_of_a_point_mass_against_itself_should_be_zero() {
        let dist = dist_from(&[(ProppFunction::Departure, 1000)]);
        assert!(cross_entropy(&dist, &dist) < EPSILON);
    }

    #[test]
    fn disjoint_distributions_should_score_at_the_floor() {
        let gold = dist_from(&[(ProppFunction::Departure, 1000)]);
        let predicted = dist_from(&[(ProppFunction::Struggle, 1000)]);

        assert_eq!(histogram_intersection(&gold, &predicted), 0.0);
        assert_eq!(top1_accuracy(&gold, &predicted), 0.0);
        // No shared support => kendall_tau's vacuous-agreement edge case. ~keep
        assert_eq!(kendall_tau(&gold, &predicted), 1.0);
    }

    /// Hand-computed values for gold = {Departure: 0.7, Struggle: 0.3} vs
    /// predicted = {Departure: 0.5, Struggle: 0.5}:
    ///
    /// - `histogram_intersection` = min(0.7,0.5) + min(0.3,0.5) = 0.5 + 0.3 = 0.8
    /// - `cross_entropy` = -(0.7*ln(0.5) + 0.3*ln(0.5)) = -ln(0.5) = 0.6931471805599453
    /// - `top1_accuracy` = 1.0 (both argmax = Departure)
    /// - `kendall_tau`: shared support {Departure, Struggle}, one pair. gold ranks
    ///   Departure > Struggle; predicted ties (0.5 == 0.5) => contributes 0 =>
    ///   tau = 0 / 1 = 0.0.
    #[test]
    fn seventy_thirty_vs_fifty_fifty_should_match_hand_computed_values() {
        let gold = dist_from(&[
            (ProppFunction::Departure, 700),
            (ProppFunction::Struggle, 300),
        ]);
        let predicted = dist_from(&[
            (ProppFunction::Departure, 500),
            (ProppFunction::Struggle, 500),
        ]);

        assert!((histogram_intersection(&gold, &predicted) - 0.8).abs() < EPSILON);
        assert!((cross_entropy(&gold, &predicted) - std::f64::consts::LN_2).abs() < EPSILON);
        assert_eq!(top1_accuracy(&gold, &predicted), 1.0);
        assert_eq!(kendall_tau(&gold, &predicted), 0.0);
    }

    #[test]
    fn kendall_tau_should_be_negative_one_for_fully_reversed_rank_order() {
        let gold = dist_from(&[
            (ProppFunction::Departure, 700),
            (ProppFunction::Struggle, 300),
        ]);
        let predicted = dist_from(&[
            (ProppFunction::Departure, 300),
            (ProppFunction::Struggle, 700),
        ]);

        assert_eq!(kendall_tau(&gold, &predicted), -1.0);
    }

    #[test]
    fn kendall_tau_of_empty_shared_support_should_be_one() {
        let gold: Dist<ProppFunction> = Dist::new();
        let predicted: Dist<ProppFunction> = Dist::new();
        assert_eq!(kendall_tau(&gold, &predicted), 1.0);
    }

    #[test]
    fn kendall_tau_of_singleton_shared_support_should_be_one() {
        let gold = dist_from(&[(ProppFunction::Departure, 1000)]);
        let predicted = dist_from(&[(ProppFunction::Departure, 500)]);
        assert_eq!(kendall_tau(&gold, &predicted), 1.0);
    }

    #[test]
    fn cross_entropy_should_stay_finite_when_predicted_misses_a_gold_key() {
        let gold = dist_from(&[(ProppFunction::Departure, 1000)]);
        let predicted = dist_from(&[(ProppFunction::Struggle, 1000)]);

        let entropy = cross_entropy(&gold, &predicted);
        assert!(entropy.is_finite());
        assert!((entropy - (-CROSS_ENTROPY_EPSILON.ln())).abs() < EPSILON);
    }

    #[test]
    fn dist_score_compute_should_bundle_all_four_metrics() {
        let dist = dist_from(&[(ProppFunction::Departure, 1000)]);
        let score = DistScore::compute(&dist, &dist);
        assert_eq!(
            score,
            DistScore {
                histogram_intersection: 1.0,
                cross_entropy: 0.0,
                top1_accuracy: 1.0,
                kendall_tau: 1.0,
            }
        );
    }
}
