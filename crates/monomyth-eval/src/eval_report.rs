//! [`EvalReport`]: the aggregate of the deterministic structural score and the
//! optional embedding-derived semantic score.
//!
//! Gated behind the default-off `semantic` feature (its only reason to exist is to
//! carry the [`SemanticReport`] axis alongside the structural [`Report`]).

use serde::Serialize;

use crate::report::Report;
use crate::semantic::SemanticReport;

/// The aggregate evaluation of a predicted world: the deterministic structural
/// [`Report`] plus the optional, embedding-derived [`SemanticReport`].
///
/// The two axes are separate fields on purpose. `structural` is a pure,
/// bit-reproducible function of its inputs and is the one pinned by
/// [`report_fingerprint`](crate::report_fingerprint); `semantic` is derived from
/// embeddings, which are not bit-stable across runs or backends, so it is
/// **never** fed to that fingerprint — the golden is unaffected whether or not
/// semantic scoring ran. A future judge axis folds in here the same way (a further
/// `Option` field), reusing this quarantine boundary rather than widening the
/// deterministic core.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct EvalReport {
    /// The deterministic structural score: node alignment plus per-axis
    /// distribution metrics.
    pub structural: Report,
    /// The prose-level semantic score, present only when semantic scoring ran.
    pub semantic: Option<SemanticReport>,
}

impl EvalReport {
    /// An [`EvalReport`] carrying only the structural score, with no semantic axis.
    ///
    /// The entry point for a caller that has not (or cannot) run embedding-based
    /// scoring: the result is structurally identical to scoring with the `semantic`
    /// feature off, so downstream fingerprinting is unaffected.
    #[must_use]
    pub fn structural_only(structural: Report) -> Self {
        Self {
            structural,
            semantic: None,
        }
    }

    /// Attach a [`SemanticReport`] to this aggregate, returning the updated report.
    #[must_use]
    pub fn with_semantic(mut self, semantic: SemanticReport) -> Self {
        self.semantic = Some(semantic);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::EvalReport;
    use crate::report::Report;
    use crate::report_fingerprint;
    use crate::semantic::SemanticReport;

    #[test]
    fn structural_only_should_carry_no_semantic_axis() {
        let report = EvalReport::structural_only(Report::default());
        assert!(report.semantic.is_none());
    }

    #[test]
    fn with_semantic_should_attach_the_axis_without_touching_structural() {
        let structural = Report::default();
        let baseline = report_fingerprint(&structural);

        let report = EvalReport::structural_only(structural).with_semantic(SemanticReport {
            pairs: 3,
            mean_cosine: 0.9,
            min_cosine: 0.7,
            max_cosine: 1.0,
        });

        assert_eq!(
            report.semantic,
            Some(SemanticReport {
                pairs: 3,
                mean_cosine: 0.9,
                min_cosine: 0.7,
                max_cosine: 1.0,
            })
        );
        // The load-bearing invariant: attaching a semantic axis leaves the
        // structural fingerprint (the frozen golden) byte-for-byte unchanged.
        assert_eq!(
            report_fingerprint(&report.structural),
            baseline,
            "the semantic axis must never perturb the structural fingerprint",
        );
    }
}
