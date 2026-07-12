//! Committed performance baselines for the non-deterministic scoring axes.
//!
//! The structural [`Report`](crate::Report) is bit-reproducible and pinned by
//! [`report_fingerprint`](crate::report_fingerprint) as an exact golden. The
//! judge, pre-score, and semantic-cosine axes are *not* bit-stable: their
//! absolute values drift with the model, the embedding backend, and sampling.
//! Pinning them to an exact hash would be a brittle, constantly-failing gate.
//!
//! Instead we summarize a *set* of observations of one axis as a [`Baseline`]
//! (count / mean / min / max / population stddev), commit it alongside the live
//! harness, and [`classify`](Baseline::classify) a fresh observation against it
//! — reporting a regression as a **soft signal to log, not a hard failure**.
//! This is the avg/min/max/stddev pattern; a live baseline tracks drift without
//! pretending a non-deterministic axis is reproducible.
//!
//! Higher is better for every axis this summarizes (judge score, pre-score
//! `overall`, mean cosine), so "below the mean by more than the tolerance" is a
//! [regression](BaselineComparison::Regressed) and "above it" is an
//! [improvement](BaselineComparison::Improved).
//!
//! Pure and dependency-free — just `f64` math over a slice, `Serialize` +
//! `Deserialize` so a committed baseline round-trips through JSON. It is never
//! observed by [`report_fingerprint`](crate::report_fingerprint), so the
//! structural golden is unaffected whether or not baselines are in play. Gated
//! behind the default-off `semantic` feature alongside the other
//! non-deterministic axes.

use serde::{Deserialize, Serialize};

/// A committed summary of one non-deterministic scoring axis over a set of
/// observations.
///
/// Built with [`Baseline::from_observations`]; compared against a fresh reading
/// with [`Baseline::classify`]. `stddev` is the **population** standard
/// deviation (divides by `count`, not `count - 1`), so a single observation has
/// `stddev == 0.0`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Baseline {
    /// The number of observations summarized (always `>= 1`; an empty set
    /// yields no baseline — see [`Baseline::from_observations`]).
    pub count: usize,
    /// The arithmetic mean of the observations.
    pub mean: f64,
    /// The smallest observation — the worst-case reading.
    pub min: f64,
    /// The largest observation — the best-case reading.
    pub max: f64,
    /// The population standard deviation of the observations; `0.0` for a
    /// single observation.
    pub stddev: f64,
}

/// How a fresh observation compares to a committed [`Baseline`], within a
/// caller-supplied tolerance band around the mean.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BaselineComparison {
    /// The observation fell more than `tolerance` *below* the baseline mean —
    /// the axis has regressed (log it; do not fail CI on a non-deterministic
    /// axis).
    Regressed,
    /// The observation is within `tolerance` of the baseline mean (either side).
    WithinTolerance,
    /// The observation exceeded the baseline mean by more than `tolerance` — a
    /// candidate improvement worth re-baselining.
    Improved,
}

impl Baseline {
    /// Summarize `observations` into a [`Baseline`], or `None` when the slice is
    /// empty (a summary of zero readings carries no information).
    ///
    /// ```
    /// use monomyth_eval::Baseline;
    ///
    /// let baseline = Baseline::from_observations(&[0.6, 0.8, 1.0]).expect("non-empty");
    /// assert_eq!(baseline.count, 3);
    /// assert!((baseline.mean - 0.8).abs() < 1e-9);
    /// assert!((baseline.min - 0.6).abs() < 1e-9);
    /// assert!((baseline.max - 1.0).abs() < 1e-9);
    ///
    /// assert!(Baseline::from_observations(&[]).is_none());
    /// ```
    #[must_use]
    pub fn from_observations(observations: &[f64]) -> Option<Self> {
        let &first = observations.first()?;
        #[allow(
            clippy::cast_precision_loss,
            reason = "observation count is a small sample count, exactly representable as f64"
        )]
        let count_f64 = observations.len() as f64;
        let mean = observations.iter().sum::<f64>() / count_f64;
        let min = observations.iter().copied().fold(first, f64::min);
        let max = observations.iter().copied().fold(first, f64::max);
        let variance = observations
            .iter()
            .map(|&value| {
                let deviation = value - mean;
                deviation * deviation
            })
            .sum::<f64>()
            / count_f64;
        Some(Self {
            count: observations.len(),
            mean,
            min,
            max,
            stddev: variance.sqrt(),
        })
    }

    /// Classify `observation` against this baseline's mean, using a non-negative
    /// `tolerance` band. An observation more than `tolerance` below the mean is
    /// [`Regressed`](BaselineComparison::Regressed); more than `tolerance` above
    /// is [`Improved`](BaselineComparison::Improved); otherwise
    /// [`WithinTolerance`](BaselineComparison::WithinTolerance).
    ///
    /// ```
    /// use monomyth_eval::{Baseline, BaselineComparison};
    ///
    /// let baseline = Baseline::from_observations(&[0.8, 0.8, 0.8]).expect("non-empty");
    /// assert_eq!(baseline.classify(0.80, 0.05), BaselineComparison::WithinTolerance);
    /// assert_eq!(baseline.classify(0.70, 0.05), BaselineComparison::Regressed);
    /// assert_eq!(baseline.classify(0.90, 0.05), BaselineComparison::Improved);
    /// ```
    #[must_use]
    pub fn classify(&self, observation: f64, tolerance: f64) -> BaselineComparison {
        if observation < self.mean - tolerance {
            BaselineComparison::Regressed
        } else if observation > self.mean + tolerance {
            BaselineComparison::Improved
        } else {
            BaselineComparison::WithinTolerance
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Baseline, BaselineComparison};

    /// Floating-point stats are compared within a tight tolerance rather than
    /// bit-for-bit (clippy's `float_cmp` forbids exact `==` on floats, and the
    /// stddev `sqrt` carries rounding anyway).
    fn assert_close(value: f64, expected: f64) {
        assert!(
            (value - expected).abs() < 1e-9,
            "expected ~{expected}, got {value}",
        );
    }

    #[test]
    fn from_observations_of_empty_slice_should_be_none() {
        assert_eq!(Baseline::from_observations(&[]), None);
    }

    #[test]
    fn from_observations_of_one_reading_should_have_zero_stddev() {
        let baseline = Baseline::from_observations(&[0.75]).expect("non-empty");
        assert_eq!(baseline.count, 1);
        assert_close(baseline.mean, 0.75);
        assert_close(baseline.min, 0.75);
        assert_close(baseline.max, 0.75);
        assert_close(baseline.stddev, 0.0);
    }

    #[test]
    fn from_observations_should_compute_mean_min_max_and_population_stddev() {
        let baseline = Baseline::from_observations(&[0.6, 0.8, 1.0]).expect("non-empty");
        assert_eq!(baseline.count, 3);
        assert_close(baseline.mean, 0.8);
        assert_close(baseline.min, 0.6);
        assert_close(baseline.max, 1.0);
        // population variance = ((-0.2)^2 + 0^2 + (0.2)^2) / 3 = 0.08/3
        assert_close(baseline.stddev, (0.08_f64 / 3.0).sqrt());
    }

    #[test]
    fn classify_within_tolerance_should_not_flag_either_direction() {
        // Powers-of-two-friendly values so the band edges (mean +/- tolerance =
        // 0.25 and 0.75) are exactly representable and edge classification is not
        // a floating-point coin flip.
        let baseline = Baseline::from_observations(&[0.5, 0.5, 0.5]).expect("non-empty");
        assert_eq!(
            baseline.classify(0.5, 0.25),
            BaselineComparison::WithinTolerance
        );
        // exactly on the band edges is within tolerance (strict inequalities).
        assert_eq!(
            baseline.classify(0.25, 0.25),
            BaselineComparison::WithinTolerance
        );
        assert_eq!(
            baseline.classify(0.75, 0.25),
            BaselineComparison::WithinTolerance
        );
    }

    #[test]
    fn classify_below_the_band_should_be_a_regression() {
        let baseline = Baseline::from_observations(&[0.8]).expect("non-empty");
        assert_eq!(baseline.classify(0.7, 0.05), BaselineComparison::Regressed);
    }

    #[test]
    fn classify_above_the_band_should_be_an_improvement() {
        let baseline = Baseline::from_observations(&[0.8]).expect("non-empty");
        assert_eq!(baseline.classify(0.95, 0.05), BaselineComparison::Improved);
    }

    #[test]
    fn baseline_should_round_trip_through_json() {
        let baseline = Baseline::from_observations(&[0.6, 0.8, 1.0]).expect("non-empty");
        let json = serde_json::to_string(&baseline).expect("serializes");
        let restored: Baseline = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(baseline, restored);
    }
}
