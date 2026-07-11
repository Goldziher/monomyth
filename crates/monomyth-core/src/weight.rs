//! [`Weight`] — an integer permille share used to score classification candidates.
//!
//! ADR-0022 generalizes every categorical framework-typed field (Propp functions,
//! Polti situations, Campbell stages, …) into a *scored* attribute: a label plus
//! one-or-more weighted values. `Weight` is the unit that distribution is measured
//! in. It is deliberately integer-only — no floating point anywhere in the
//! contract — so serialization stays bit-exact and a play session's
//! `(seed, action-log)` replay guarantee (ADR-0002/ADR-0003) is never at the mercy
//! of platform rounding.

use serde::{Deserialize, Deserializer, Serialize};

/// The upper bound of the permille scale: a [`Weight`] represents a share of this.
///
/// Named so `1000` never appears as a bare magic number elsewhere in this file.
const MAX_PERMILLE: u16 = 1000;

/// An integer permille share in `0..=1000`, used to score a classification
/// candidate (e.g. how strongly a narrative beat expresses a given Propp
/// function).
///
/// Weights are relative shares within a [`ScoredSet`](crate::ScoredSet) or
/// [`ScoredOne`](crate::ScoredOne); they need not sum to `1000` across a set —
/// normalization happens at draw time (`draw_weighted_index`), not at
/// construction. Keeping the type integer-only avoids reintroducing float
/// nondeterminism into a contract whose serialized form must be bit-exact.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(transparent)]
pub struct Weight(u16);

impl Weight {
    /// The maximum weight: a full, undivided share.
    pub const FULL: Self = Self(MAX_PERMILLE);

    /// The minimum weight: no share at all.
    pub const ZERO: Self = Self(0);

    /// Build a `Weight` from a raw permille value, clamping anything above
    /// [`MAX_PERMILLE`] down to it.
    ///
    /// This clamps rather than returning a `Result`. Generation-time values can
    /// arrive from arbitrary sources (crosswalk artifacts, extraction output,
    /// RNG-driven procedural passes) and a `Weight` must always be constructible
    /// without threading fallible plumbing through every call site that builds
    /// one; clamping to the valid range is a safe, deterministic, and total
    /// operation that preserves the "relative share" semantics (a share can
    /// never exceed "all of it").
    ///
    /// ```
    /// use monomyth_core::Weight;
    ///
    /// assert_eq!(Weight::new(250).permille(), 250);
    /// assert_eq!(Weight::new(1500).permille(), 1000);
    /// ```
    #[must_use]
    pub const fn new(permille: u16) -> Self {
        if permille > MAX_PERMILLE {
            Self(MAX_PERMILLE)
        } else {
            Self(permille)
        }
    }

    /// The raw permille value, always in `0..=1000`.
    #[must_use]
    pub const fn permille(self) -> u16 {
        self.0
    }
}

/// The default weight is [`Weight::FULL`], not the derived all-zero value: an
/// unscored candidate should read as a full, undivided share rather than no
/// share at all, matching the `From<BTreeSet<T>>` migration bridge on
/// [`ScoredSet`](crate::ScoredSet).
impl Default for Weight {
    fn default() -> Self {
        Self::FULL
    }
}

/// Deserialization must uphold the same `0..=1000` invariant as [`Weight::new`].
///
/// `#[serde(transparent)]` on a derived `Deserialize` would accept any `u16`
/// straight into the tuple field, letting a stored out-of-range value (e.g. a
/// hand-edited artifact with `1500`) survive deserialization unclamped. A manual
/// impl that reads a `u16` and routes it through `Weight::new` keeps
/// deserialization and construction the same total, clamping operation.
impl<'de> Deserialize<'de> for Weight {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let permille = u16::deserialize(deserializer)?;
        Ok(Self::new(permille))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_should_pass_through_values_within_range_unchanged() {
        assert_eq!(Weight::new(250).permille(), 250);
        assert_eq!(Weight::new(1000).permille(), 1000);
    }

    #[test]
    fn new_should_keep_zero_as_zero() {
        assert_eq!(Weight::new(0).permille(), 0);
    }

    #[test]
    fn new_should_clamp_values_above_max_down_to_exactly_1000() {
        assert_eq!(Weight::new(1001).permille(), 1000);
        assert_eq!(Weight::new(1500).permille(), 1000);
        assert_eq!(Weight::new(u16::MAX).permille(), 1000);
    }

    #[test]
    fn default_should_equal_full_not_derived_zero() {
        assert_eq!(Weight::default(), Weight::FULL);
        assert_eq!(Weight::default().permille(), 1000);
    }

    #[test]
    fn full_and_zero_consts_should_hold_their_named_bounds() {
        assert_eq!(Weight::FULL.permille(), 1000);
        assert_eq!(Weight::ZERO.permille(), 0);
    }

    #[test]
    fn serialize_should_produce_a_bare_json_integer() {
        assert_eq!(serde_json::to_string(&Weight::new(250)).unwrap(), "250");
        assert_eq!(serde_json::to_string(&Weight::FULL).unwrap(), "1000");
        assert_eq!(serde_json::to_string(&Weight::ZERO).unwrap(), "0");
    }

    #[test]
    fn deserialize_should_round_trip_an_in_range_value() {
        let weight: Weight = serde_json::from_str("250").unwrap();
        assert_eq!(weight, Weight::new(250));
    }

    #[test]
    fn deserialize_should_clamp_out_of_range_input_instead_of_erroring() {
        let weight: Weight = serde_json::from_str("1500").unwrap();
        assert_eq!(weight.permille(), 1000);
        assert_eq!(weight, Weight::new(1000));
    }

    /// A doctest-style round trip using `?`, verified here as a regular test since
    /// it returns a `Result` from a fallible operation (deserialization).
    #[test]
    fn deserialize_round_trip_with_question_mark_operator() -> Result<(), serde_json::Error> {
        let weight: Weight = serde_json::from_str("1500")?;
        assert_eq!(weight.permille(), 1000);
        Ok(())
    }
}
