//! [`RagSoftmaxClassifier`] — a [`Classifier`] over a fused retrieval query.
//!
//! For each candidate stage, the classifier fuses the input text with the
//! stage's own name/description into one query, retrieves scored evidence for
//! that fused query, and sums the scores. A softmax over the 17 per-stage
//! evidence totals turns "how much evidence did this fused query surface" into a
//! probability distribution, which is then converted to the [`ScoredOne`]
//! shape the rest of the contract expects.

use std::collections::BTreeMap;

use async_trait::async_trait;

use monomyth_contracts::{Classifier, ClassifyError, PassageRetriever};
use monomyth_core::{ScoredOne, Weight};
use monomyth_frameworks::MonomythStage;

/// The number of scored hits requested per fused per-stage query.
///
/// 8 is a small-but-not-degenerate window: large enough that a single
/// outlier hit cannot dominate a stage's evidence total, small enough that a
/// hermetic test corpus (a handful of fixture passages) does not need to be
/// padded just to fill the window. Production tuning of this knob against a
/// real corpus is future work; the proof-of-concept only needs a fixed,
/// documented value.
const DEFAULT_TOP_K: u32 = 8;

/// Softmax temperature applied to the per-stage evidence totals.
///
/// `1.0` is the untempered softmax: it neither sharpens (temperature `< 1.0`)
/// nor flattens (temperature `> 1.0`) the distribution relative to the raw
/// evidence totals, so the resulting probabilities are a direct, unbiased
/// read of "how much more evidence did this stage's fused query surface than
/// the others" without an extra unjustified hyperparameter. A future
/// calibration pass against a labeled corpus may retune this; until then, the
/// neutral value keeps the proof-of-concept's behavior easy to reason about.
const SOFTMAX_TEMPERATURE: f64 = 1.0;

/// Score `text` against every [`MonomythStage`] by fusing it with each stage's
/// own descriptor and retrieving evidence for the fused query through `R`.
///
/// # Determinism
///
/// The *written* output is an integer permille [`Weight`] per alternative
/// (and an argmax `primary`), and downstream `report_fingerprint` quantizes
/// every score to permille precision before hashing. Floating-point softmax
/// probabilities are strictly an intermediate: sub-permille `exp()` platform
/// noise cannot drift a golden fingerprint built on this classifier's output,
/// because that noise never survives the permille rounding on the way to a
/// [`Weight`].
#[derive(Debug, Clone, Copy)]
pub struct RagSoftmaxClassifier<R: PassageRetriever> {
    retriever: R,
    top_k: u32,
}

impl<R: PassageRetriever> RagSoftmaxClassifier<R> {
    /// Build a classifier over `retriever`, using [`DEFAULT_TOP_K`] hits per
    /// fused per-stage query.
    #[must_use]
    pub const fn new(retriever: R) -> Self {
        Self::with_top_k(retriever, DEFAULT_TOP_K)
    }

    /// Build a classifier over `retriever`, requesting `top_k` hits per fused
    /// per-stage query.
    #[must_use]
    pub const fn with_top_k(retriever: R, top_k: u32) -> Self {
        Self { retriever, top_k }
    }

    /// Build the fused query for one candidate `stage` against `text`: the
    /// input text followed by the stage's own name and description, so
    /// retrieval scores how well the corpus supports *this stage reading* of
    /// `text` rather than `text` alone.
    fn fused_query(text: &str, stage: MonomythStage) -> String {
        let info = stage.info();
        let anchor = format!("{}. {}", info.name, info.description);
        format!("{text} {anchor}")
    }

    /// Gather the total retrieval evidence for every [`MonomythStage`],
    /// alongside the total hit count across all fused queries.
    async fn gather_evidence(
        &self,
        text: &str,
    ) -> Result<(BTreeMap<MonomythStage, f64>, usize), ClassifyError> {
        let mut evidence = BTreeMap::new();
        let mut total_hits = 0_usize;

        for &stage in MonomythStage::all() {
            let query = Self::fused_query(text, stage);
            let hits = self.retriever.retrieve_scores(&query, self.top_k).await?;
            total_hits += hits.len();
            let total_score: f64 = hits.iter().map(|hit| f64::from(hit.score)).sum();
            evidence.insert(stage, total_score);
        }

        Ok((evidence, total_hits))
    }
}

/// Softmax `evidence`'s values at [`SOFTMAX_TEMPERATURE`], subtracting the max
/// before `exp` for numerical stability.
///
/// `evidence` must be non-empty; callers here always pass all 17
/// [`MonomythStage`] variants.
fn softmax(evidence: &BTreeMap<MonomythStage, f64>) -> BTreeMap<MonomythStage, f64> {
    let max = evidence.values().copied().fold(f64::NEG_INFINITY, f64::max);

    let exponentiated: BTreeMap<MonomythStage, f64> = evidence
        .iter()
        .map(|(&stage, &value)| (stage, ((value - max) / SOFTMAX_TEMPERATURE).exp()))
        .collect();
    let sum: f64 = exponentiated.values().sum();

    exponentiated
        .into_iter()
        .map(|(stage, value)| (stage, value / sum))
        .collect()
}

/// Convert a per-stage probability distribution into a [`ScoredOne`]: the
/// argmax as `primary` (ties broken toward the canonically smallest
/// [`MonomythStage`], since `probabilities` iterates in `BTreeMap`/`Ord`
/// order and this keeps only strictly-greater candidates), and every other
/// stage whose rounded permille weight is at least 1 as a weighted
/// alternative.
fn to_scored_one(probabilities: &BTreeMap<MonomythStage, f64>) -> ScoredOne<MonomythStage> {
    let mut primary = MonomythStage::all()[0];
    let mut primary_probability = f64::NEG_INFINITY;
    for (&stage, &probability) in probabilities {
        if probability > primary_probability {
            primary = stage;
            primary_probability = probability;
        }
    }

    let mut scored = ScoredOne::new(primary);
    for (&stage, &probability) in probabilities {
        if stage == primary {
            continue;
        }
        #[allow(
            clippy::cast_possible_truncation,
            reason = "a softmax probability is bounded to 0.0..=1.0, so its permille rounding is bounded to 0..=1000"
        )]
        #[allow(
            clippy::cast_sign_loss,
            reason = "a softmax probability is never negative, so the permille rounding is never negative"
        )]
        let permille = (probability * 1000.0).round() as u16;
        if permille >= 1 {
            scored.insert_alternative(stage, Weight::new(permille));
        }
    }
    scored
}

#[async_trait]
impl<R: PassageRetriever> Classifier<MonomythStage> for RagSoftmaxClassifier<R> {
    async fn classify(&self, text: &str) -> Result<ScoredOne<MonomythStage>, ClassifyError> {
        let (evidence, total_hits) = self.gather_evidence(text).await?;
        if total_hits == 0 {
            return Err(ClassifyError::NoEvidence);
        }

        let probabilities = softmax(&evidence);
        Ok(to_scored_one(&probabilities))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use monomyth_contracts::{RetrievalError, ScoredHit};

    /// Returns exactly one hit whose score is `1.0` when the fused query
    /// contains `target`'s canonical name and `0.0` otherwise — so only the
    /// target stage's own fused query accrues evidence, and it must win the
    /// argmax.
    struct ScriptedRetriever {
        target: MonomythStage,
    }

    #[async_trait]
    impl PassageRetriever for ScriptedRetriever {
        async fn retrieve_scores(
            &self,
            query: &str,
            _top_k: u32,
        ) -> Result<Vec<ScoredHit>, RetrievalError> {
            let score = if query.contains(&self.target.info().name) {
                1.0
            } else {
                0.0
            };
            Ok(vec![ScoredHit { score }])
        }
    }

    /// Returns no hits for any query — the empty-corpus case.
    struct EmptyRetriever;

    #[async_trait]
    impl PassageRetriever for EmptyRetriever {
        async fn retrieve_scores(
            &self,
            _query: &str,
            _top_k: u32,
        ) -> Result<Vec<ScoredHit>, RetrievalError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn classify_should_pick_the_stage_whose_fused_query_scores_highest() {
        // `CrossingTheFirstThreshold` has a distinctive canonical name that no
        // other stage's fused query contains, so the scripted retriever gives
        // evidence to that stage alone.
        let target = MonomythStage::CrossingTheFirstThreshold;
        let classifier = RagSoftmaxClassifier::new(ScriptedRetriever { target });

        let scored = classifier
            .classify("a hero steps past the guardians at the border")
            .await
            .expect("evidence is non-empty, so classification succeeds");

        assert_eq!(*scored.primary(), target);
    }

    #[tokio::test]
    async fn classify_should_never_list_the_primary_among_its_alternatives() {
        let target = MonomythStage::CrossingTheFirstThreshold;
        let classifier = RagSoftmaxClassifier::new(ScriptedRetriever { target });

        let scored = classifier
            .classify("a threshold crossing")
            .await
            .expect("succeeds");

        assert!(
            !scored.alternatives().contains_key(scored.primary()),
            "the primary must never also appear as an alternative"
        );
    }

    #[tokio::test]
    async fn classify_alternative_weights_should_stay_in_the_permille_range() {
        let classifier = RagSoftmaxClassifier::new(ScriptedRetriever {
            target: MonomythStage::TheRoadOfTrials,
        });

        let scored = classifier.classify("a trial").await.expect("succeeds");

        for weight in scored.alternatives().values() {
            let permille = weight.permille();
            assert!(
                (1..=1000).contains(&permille),
                "alternative weight {permille} out of the 1..=1000 permille range"
            );
        }
    }

    #[tokio::test]
    async fn classify_should_report_no_evidence_when_the_corpus_is_empty() {
        let classifier = RagSoftmaxClassifier::new(EmptyRetriever);

        let error = classifier
            .classify("nothing will match")
            .await
            .expect_err("an empty corpus cannot classify");

        assert!(matches!(error, ClassifyError::NoEvidence));
    }
}
