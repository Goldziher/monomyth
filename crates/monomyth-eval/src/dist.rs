//! [`Dist`] — a discrete distribution over an attribute's values.
//!
//! ADR-0023's scoring harness compares a gold classification against a predicted
//! one as distributions, not bare labels, so it can measure how *close* a
//! prediction is rather than only whether it is exactly right. `Dist` is the
//! common shape both gold fixtures and predicted output are converted into
//! before any metric runs.
//!
//! Unlike `monomyth-core`'s [`Weight`](monomyth_core::Weight) (an integer
//! permille, deliberately float-free so the *contract* stays bit-exact), `Dist`
//! is scoring-only: it lives entirely in this crate, never crosses back into the
//! serialized world, and normalizes to `f64` shares that sum to `1.0`. Floating
//! point here does not threaten replay determinism because a `Dist` is derived
//! output, not stored state.

use std::collections::BTreeMap;

use monomyth_core::{ScoredOne, ScoredSet, Weight};

/// A discrete probability distribution over an attribute's possible values,
/// normalized so its weights sum to `1.0` (or the distribution is empty).
///
/// Backed by a `BTreeMap` so iteration is in canonical `T`-order, matching the
/// project-wide discipline of deterministic collections.
#[derive(Clone, Debug, PartialEq)]
pub struct Dist<T: Ord + Clone>(BTreeMap<T, f64>);

impl<T: Ord + Clone> Dist<T> {
    /// An empty distribution: no support, no mass.
    #[must_use]
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Build a normalized `Dist` from raw, not-necessarily-normalized weights.
    ///
    /// Entries with zero total weight collapse to the empty distribution rather
    /// than dividing by zero. Values already in `0.0..=1.0` that sum to `1.0` are
    /// preserved bit-for-bit only up to floating-point division identity (`x / 1.0
    /// == x`), which holds exactly in IEEE 754.
    fn from_raw_weights(raw: BTreeMap<T, f64>) -> Self {
        let total: f64 = raw.values().sum();
        if total <= 0.0 {
            return Self::new();
        }
        Self(
            raw.into_iter()
                .map(|(key, weight)| (key, weight / total))
                .collect(),
        )
    }

    /// The probability mass on `key`, or `0.0` if it is not in the support.
    #[must_use]
    pub fn get(&self, key: &T) -> f64 {
        self.0.get(key).copied().unwrap_or(0.0)
    }

    /// Iterate `(value, probability)` pairs in canonical `T`-order.
    pub fn iter(&self) -> impl Iterator<Item = (&T, f64)> {
        self.0.iter().map(|(key, probability)| (key, *probability))
    }

    /// The distribution's support: every value with nonzero probability, in
    /// canonical `T`-order.
    pub fn support(&self) -> impl Iterator<Item = &T> {
        self.0.keys()
    }

    /// Whether this distribution has no support (no values were ever scored, or
    /// every raw weight was zero).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number of values in the support.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// The value(s) with the highest probability, in canonical `T`-order.
    ///
    /// Deterministic tie-breaking: when two or more values share the maximum
    /// probability, the canonically smallest `T` wins, so `argmax` never depends
    /// on iteration or insertion order.
    #[must_use]
    pub fn argmax(&self) -> Option<&T> {
        self.0
            .iter()
            .max_by(|(a_key, a_prob), (b_key, b_prob)| {
                a_prob
                    .partial_cmp(b_prob)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    // Reverse the key comparison so that on a probability tie, ~keep
                    // `max_by` (which keeps the *last* maximal element) keeps the ~keep
                    // canonically *smallest* key. ~keep
                    .then(b_key.cmp(a_key))
            })
            .map(|(key, _)| key)
    }
}

impl<T: Ord + Clone> Default for Dist<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert a [`ScoredSet`] into a normalized `Dist`: each candidate's permille
/// [`Weight`] becomes its raw weight before normalization.
impl<T: Ord + Clone> From<&ScoredSet<T>> for Dist<T> {
    fn from(set: &ScoredSet<T>) -> Self {
        let raw: BTreeMap<T, f64> = set
            .iter()
            .map(|(key, weight)| (key.clone(), f64::from(weight.permille())))
            .collect();
        Self::from_raw_weights(raw)
    }
}

/// Convert a [`ScoredOne`] into a normalized `Dist`.
///
/// # Normalization rule for the primary
///
/// [`ScoredOne::primary`] carries no explicit [`Weight`] of its own — it is the
/// classification's mandatory label, and `alternatives` scores weaker competing
/// readings. This conversion assigns the primary [`Weight::FULL`] (the same
/// implicit weight the `BTreeSet<T> -> ScoredSet<T>` migration bridge in
/// `monomyth-core` gives an unweighted element), *unless* the primary also
/// appears in `alternatives` — which [`ScoredOne`]'s type-level invariant forbids
/// for values built through its typed API, but a derived `Deserialize` cannot
/// enforce (see
/// [`primary_duplicated_in_alternatives`](ScoredOne::primary_duplicated_in_alternatives)).
/// In that malformed case, the alternative's explicit weight wins rather than
/// being silently summed with the implicit `FULL`, so a `Dist` built from an
/// invalid `ScoredOne` never exceeds one unit of mass at that key. Callers should
/// prefer running [`World::validate`](monomyth_core::World::validate) before
/// converting, which rejects that malformed shape at the load boundary.
impl<T: Ord + Clone> From<&ScoredOne<T>> for Dist<T> {
    fn from(scored: &ScoredOne<T>) -> Self {
        let mut raw: BTreeMap<T, f64> = scored
            .alternatives()
            .iter()
            .map(|(key, weight)| (key.clone(), f64::from(weight.permille())))
            .collect();
        raw.entry(scored.primary().clone())
            .or_insert(f64::from(Weight::FULL.permille()));
        Self::from_raw_weights(raw)
    }
}

#[cfg(test)]
// `dist.get(...)` on an absent key returns the exact literal `0.0` (see ~keep
// `get`'s `unwrap_or(0.0)`), not an accumulated float, so an exact comparison ~keep
// is the correct assertion there; ratios computed by normalization are still ~keep
// compared with `EPSILON` below. ~keep
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;
    use monomyth_frameworks::ProppFunction;

    const EPSILON: f64 = 1e-12;

    #[test]
    fn new_dist_should_be_empty() {
        let dist: Dist<ProppFunction> = Dist::new();
        assert!(dist.is_empty());
        assert_eq!(dist.len(), 0);
        assert_eq!(dist.get(&ProppFunction::Departure), 0.0);
    }

    #[test]
    fn from_scored_set_should_normalize_weights_to_sum_to_one() {
        let mut set = ScoredSet::new();
        set.insert(ProppFunction::Departure, Weight::new(700));
        set.insert(ProppFunction::Struggle, Weight::new(300));

        let dist = Dist::from(&set);
        assert!((dist.get(&ProppFunction::Departure) - 0.7).abs() < EPSILON);
        assert!((dist.get(&ProppFunction::Struggle) - 0.3).abs() < EPSILON);
        assert_eq!(dist.get(&ProppFunction::Victory), 0.0);
    }

    #[test]
    fn from_scored_set_with_unnormalized_weights_should_still_sum_to_one() {
        let mut set = ScoredSet::new();
        set.insert(ProppFunction::Departure, Weight::new(1000));
        set.insert(ProppFunction::Struggle, Weight::new(1000));

        let dist = Dist::from(&set);
        assert!((dist.get(&ProppFunction::Departure) - 0.5).abs() < EPSILON);
        assert!((dist.get(&ProppFunction::Struggle) - 0.5).abs() < EPSILON);
    }

    #[test]
    fn from_empty_scored_set_should_produce_an_empty_dist() {
        let set: ScoredSet<ProppFunction> = ScoredSet::new();
        let dist = Dist::from(&set);
        assert!(dist.is_empty());
    }

    #[test]
    fn from_scored_one_with_no_alternatives_should_give_primary_all_the_mass() {
        let scored = ScoredOne::new(ProppFunction::Departure);
        let dist = Dist::from(&scored);
        assert_eq!(dist.len(), 1);
        assert!((dist.get(&ProppFunction::Departure) - 1.0).abs() < EPSILON);
    }

    #[test]
    fn from_scored_one_should_weigh_primary_as_full_against_alternatives() {
        let mut scored = ScoredOne::new(ProppFunction::Departure);
        scored.insert_alternative(ProppFunction::Struggle, Weight::new(500));

        // primary implicit weight = FULL = 1000; alternative = 500; total = 1500. ~keep
        let dist = Dist::from(&scored);
        assert!((dist.get(&ProppFunction::Departure) - (1000.0 / 1500.0)).abs() < EPSILON);
        assert!((dist.get(&ProppFunction::Struggle) - (500.0 / 1500.0)).abs() < EPSILON);
    }

    #[test]
    fn argmax_should_break_ties_by_canonical_order() {
        let mut set = ScoredSet::new();
        set.insert(ProppFunction::Victory, Weight::new(500));
        set.insert(ProppFunction::Departure, Weight::new(500));

        let dist = Dist::from(&set);
        // ProppFunction::Departure < ProppFunction::Victory in artifact id order. ~keep
        assert_eq!(dist.argmax(), Some(&ProppFunction::Departure));
    }

    #[test]
    fn argmax_of_empty_dist_should_be_none() {
        let dist: Dist<ProppFunction> = Dist::new();
        assert_eq!(dist.argmax(), None);
    }

    #[test]
    fn support_should_yield_keys_in_canonical_order() {
        let mut set = ScoredSet::new();
        set.insert(ProppFunction::Victory, Weight::new(100));
        set.insert(ProppFunction::Departure, Weight::new(200));

        let dist = Dist::from(&set);
        let support: Vec<&ProppFunction> = dist.support().collect();
        assert_eq!(
            support,
            vec![&ProppFunction::Departure, &ProppFunction::Victory]
        );
    }
}
