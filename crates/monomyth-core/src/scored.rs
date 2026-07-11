//! [`ScoredSet`] and [`ScoredOne`] — the two scored-attribute containers of ADR-0022.
//!
//! Real scholarship classifies a narrative element as a *distribution* over
//! categories, not a single best-guess label — a beat might be 70% one Propp
//! function, 30% another. These two typed containers replace bare
//! `BTreeSet<T>`/`Option<T>`/`T` framework fields with a label plus weighted
//! alternatives, so generation can favor a stronger-scored candidate instead of
//! drawing uniformly, while staying grounded to a `T: Ord` framework enum (typed,
//! not a stringly-keyed bag) and keeping `BTreeMap`'s canonical ordering, which is
//! load-bearing for stable, snapshot-testable serialization (a hard project
//! invariant: deterministic collections, never `Hash*`).
//!
//! This module is purely additive for Phase S0: no existing field is changed to
//! use these types yet, and `SCHEMA_VERSION` is not bumped.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::Weight;

/// A zero-or-more scored classification axis: each candidate `T` carries its own
/// [`Weight`].
///
/// Used where a narrative element can express several categories at once (e.g.
/// `NarrativeNode.functions`, `.motifs`). Backed by a `BTreeMap` so iteration and
/// serialization are in canonical key order regardless of insertion order.
#[derive(Clone, Debug, Eq, PartialEq, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScoredSet<T: Ord>(BTreeMap<T, Weight>);

impl<T: Ord> ScoredSet<T> {
    /// An empty scored set.
    #[must_use]
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// Insert `key` with `weight`, returning the previous weight if `key` was
    /// already present (matching `BTreeMap::insert` semantics).
    pub fn insert(&mut self, key: T, weight: Weight) -> Option<Weight> {
        self.0.insert(key, weight)
    }

    /// The weight scored for `key`, or `None` if it is not a member.
    #[must_use]
    pub fn get(&self, key: &T) -> Option<Weight> {
        self.0.get(key).copied()
    }

    /// Whether `key` is a member of this set.
    #[must_use]
    pub fn contains(&self, key: &T) -> bool {
        self.0.contains_key(key)
    }

    /// The number of scored candidates.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether this set has no scored candidates.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterate `(candidate, weight)` pairs in canonical `T`-order.
    pub fn iter(&self) -> impl Iterator<Item = (&T, Weight)> {
        self.0.iter().map(|(key, weight)| (key, *weight))
    }

    /// Iterate the scored candidates in canonical `T`-order.
    pub fn keys(&self) -> impl Iterator<Item = &T> {
        self.0.keys()
    }
}

impl<T: Ord> FromIterator<(T, Weight)> for ScoredSet<T> {
    fn from_iter<I: IntoIterator<Item = (T, Weight)>>(iter: I) -> Self {
        Self(BTreeMap::from_iter(iter))
    }
}

impl<T: Ord> Extend<(T, Weight)> for ScoredSet<T> {
    fn extend<I: IntoIterator<Item = (T, Weight)>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

/// The v2 → v3 migration bridge: every element of an unweighted `BTreeSet<T>`
/// becomes a full-weight candidate.
///
/// This is how a pre-ADR-0022 field (`BTreeSet<ProppFunction>`, unweighted) lifts
/// into the scored representation without losing information — every element was
/// implicitly "fully" a member, so it keeps [`Weight::FULL`].
impl<T: Ord> From<BTreeSet<T>> for ScoredSet<T> {
    fn from(set: BTreeSet<T>) -> Self {
        set.into_iter().map(|key| (key, Weight::FULL)).collect()
    }
}

/// An exactly-one scored classification axis: a mandatory `primary` label plus
/// zero-or-more scored `alternatives`.
///
/// Used where a narrative element must always carry one label but scholarship may
/// also record weaker competing readings (e.g. `NarrativeNode.stage`,
/// `Entity.role`). `primary` has no explicit weight of its own — it is the
/// classification's chosen label; `alternatives` scores anything else that was
/// considered.
///
/// # Invariant: `primary` never also appears in `alternatives`
///
/// A candidate equal to `primary` in `alternatives` would be redundant (it
/// already has an implicit primary status) and contradictory (is it the label or
/// a competitor?). This phase enforces the invariant locally, in
/// [`insert_alternative`](Self::insert_alternative); wiring it into
/// `World::validate` is a later phase.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScoredOne<T: Ord> {
    primary: T,
    alternatives: BTreeMap<T, Weight>,
}

impl<T: Ord> ScoredOne<T> {
    /// Build a `ScoredOne` with `primary` as the label and no alternatives.
    #[must_use]
    pub fn new(primary: T) -> Self {
        Self {
            primary,
            alternatives: BTreeMap::new(),
        }
    }

    /// The mandatory primary label.
    #[must_use]
    pub const fn primary(&self) -> &T {
        &self.primary
    }

    /// The scored alternative candidates, in canonical `T`-order.
    #[must_use]
    pub const fn alternatives(&self) -> &BTreeMap<T, Weight> {
        &self.alternatives
    }

    /// Score `key` as an alternative with `weight`.
    ///
    /// Rejected as a no-op when `key == primary` (see the type-level invariant
    /// doc): the call returns `None` and `alternatives` is left unchanged. When
    /// `key` differs from `primary`, this behaves like `BTreeMap::insert` and
    /// returns the previous weight, if any.
    pub fn insert_alternative(&mut self, key: T, weight: Weight) -> Option<Weight> {
        if key == self.primary {
            return None;
        }
        self.alternatives.insert(key, weight)
    }
}

/// Wrap a bare value as the primary label with no scored alternatives.
impl<T: Ord> From<T> for ScoredOne<T> {
    fn from(primary: T) -> Self {
        Self::new(primary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monomyth_frameworks::ProppFunction;

    #[test]
    fn scored_set_new_should_start_empty() {
        let set: ScoredSet<ProppFunction> = ScoredSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn scored_set_insert_should_return_previous_weight_on_overwrite() {
        let mut set = ScoredSet::new();
        assert_eq!(set.insert(ProppFunction::Departure, Weight::new(500)), None);
        assert_eq!(
            set.insert(ProppFunction::Departure, Weight::new(750)),
            Some(Weight::new(500))
        );
        assert_eq!(set.get(&ProppFunction::Departure), Some(Weight::new(750)));
    }

    #[test]
    fn scored_set_contains_and_get_should_reflect_membership() {
        let mut set = ScoredSet::new();
        set.insert(ProppFunction::Departure, Weight::new(300));
        assert!(set.contains(&ProppFunction::Departure));
        assert!(!set.contains(&ProppFunction::Return));
        assert_eq!(set.get(&ProppFunction::Return), None);
    }

    #[test]
    fn scored_set_from_btree_set_should_give_every_element_full_weight() {
        let source = BTreeSet::from([
            ProppFunction::Departure,
            ProppFunction::Struggle,
            ProppFunction::Victory,
        ]);
        let scored: ScoredSet<ProppFunction> = source.into();

        assert_eq!(scored.len(), 3);
        assert_eq!(scored.get(&ProppFunction::Departure), Some(Weight::FULL));
        assert_eq!(scored.get(&ProppFunction::Struggle), Some(Weight::FULL));
        assert_eq!(scored.get(&ProppFunction::Victory), Some(Weight::FULL));
    }

    #[test]
    fn scored_set_iter_should_yield_canonical_btreemap_order_regardless_of_insertion_order() {
        let mut set = ScoredSet::new();
        // Insert out of Ord order (Victory=18, Departure=11, Struggle=16).
        set.insert(ProppFunction::Victory, Weight::new(100));
        set.insert(ProppFunction::Departure, Weight::new(200));
        set.insert(ProppFunction::Struggle, Weight::new(300));

        let keys: Vec<ProppFunction> = set.keys().copied().collect();
        assert_eq!(
            keys,
            vec![
                ProppFunction::Departure,
                ProppFunction::Struggle,
                ProppFunction::Victory,
            ]
        );
    }

    #[test]
    fn scored_set_from_iterator_and_extend_should_build_expected_membership() {
        let mut set: ScoredSet<ProppFunction> = ScoredSet::from_iter([
            (ProppFunction::Departure, Weight::new(400)),
            (ProppFunction::Struggle, Weight::new(600)),
        ]);
        set.extend([(ProppFunction::Victory, Weight::new(1000))]);

        assert_eq!(set.len(), 3);
        assert_eq!(set.get(&ProppFunction::Departure), Some(Weight::new(400)));
        assert_eq!(set.get(&ProppFunction::Struggle), Some(Weight::new(600)));
        assert_eq!(set.get(&ProppFunction::Victory), Some(Weight::FULL));
    }

    #[test]
    fn scored_set_serde_round_trip_should_preserve_membership_and_weights() {
        let set: ScoredSet<ProppFunction> = ScoredSet::from_iter([
            (ProppFunction::Departure, Weight::new(400)),
            (ProppFunction::Struggle, Weight::new(600)),
        ]);

        let json = serde_json::to_string(&set).unwrap();
        let round_tripped: ScoredSet<ProppFunction> = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, set);
    }

    #[test]
    fn scored_set_serde_should_produce_exact_json_snapshot() {
        let set: ScoredSet<ProppFunction> = ScoredSet::from_iter([
            (ProppFunction::Victory, Weight::new(1000)),
            (ProppFunction::Departure, Weight::new(700)),
            (ProppFunction::Struggle, Weight::new(300)),
        ]);

        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(
            json, r#"{"Departure":700,"Struggle":300,"Victory":1000}"#,
            "keys must serialize in canonical BTreeMap order, not insertion order"
        );
    }

    #[test]
    fn scored_one_from_should_produce_primary_with_empty_alternatives() {
        let scored: ScoredOne<ProppFunction> = ProppFunction::Departure.into();
        assert_eq!(scored.primary(), &ProppFunction::Departure);
        assert!(scored.alternatives().is_empty());
    }

    #[test]
    fn scored_one_new_should_produce_primary_with_empty_alternatives() {
        let scored = ScoredOne::new(ProppFunction::Departure);
        assert_eq!(scored.primary(), &ProppFunction::Departure);
        assert_eq!(scored.alternatives().len(), 0);
    }

    #[test]
    fn scored_one_insert_alternative_should_add_a_distinct_candidate() {
        let mut scored = ScoredOne::new(ProppFunction::Departure);
        let previous = scored.insert_alternative(ProppFunction::Struggle, Weight::new(250));
        assert_eq!(previous, None);
        assert_eq!(
            scored.alternatives().get(&ProppFunction::Struggle),
            Some(&Weight::new(250))
        );
    }

    #[test]
    fn scored_one_insert_alternative_should_reject_key_equal_to_primary() {
        let mut scored = ScoredOne::new(ProppFunction::Departure);
        let result = scored.insert_alternative(ProppFunction::Departure, Weight::new(900));

        assert_eq!(
            result, None,
            "rejected insert must report None, not the primary's implicit weight"
        );
        assert!(
            !scored
                .alternatives()
                .contains_key(&ProppFunction::Departure),
            "primary must never also appear in alternatives"
        );
        assert!(scored.alternatives().is_empty());
    }

    #[test]
    fn scored_one_insert_alternative_should_return_previous_weight_on_overwrite() {
        let mut scored = ScoredOne::new(ProppFunction::Departure);
        scored.insert_alternative(ProppFunction::Struggle, Weight::new(250));
        let previous = scored.insert_alternative(ProppFunction::Struggle, Weight::new(500));
        assert_eq!(previous, Some(Weight::new(250)));
        assert_eq!(
            scored.alternatives().get(&ProppFunction::Struggle),
            Some(&Weight::new(500))
        );
    }

    #[test]
    fn scored_one_serde_round_trip_should_preserve_primary_and_alternatives() {
        let mut scored = ScoredOne::new(ProppFunction::Departure);
        scored.insert_alternative(ProppFunction::Struggle, Weight::new(250));

        let json = serde_json::to_string(&scored).unwrap();
        let round_tripped: ScoredOne<ProppFunction> = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped, scored);
    }

    #[test]
    fn scored_one_serde_should_produce_exact_json_snapshot() {
        let mut scored = ScoredOne::new(ProppFunction::Departure);
        scored.insert_alternative(ProppFunction::Victory, Weight::new(300));
        scored.insert_alternative(ProppFunction::Struggle, Weight::new(150));

        let json = serde_json::to_string(&scored).unwrap();
        assert_eq!(
            json,
            r#"{"primary":"Departure","alternatives":{"Struggle":150,"Victory":300}}"#
        );
    }
}
