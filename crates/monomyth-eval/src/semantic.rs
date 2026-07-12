//! Prose-level semantic scoring: an embedding-cosine axis complementing the
//! deterministic structural [`Report`](crate::Report).
//!
//! Where [`AlignmentScorer`](crate::AlignmentScorer) scores the *structure* of a
//! generated world against a gold fixture with bit-stable distribution metrics,
//! this axis scores generated *prose* by cosine similarity of its embeddings.
//! Embeddings are not bit-reproducible across runs or backends, so this axis is
//! deliberately quarantined from [`report_fingerprint`](crate::report_fingerprint)
//! (the frozen determinism golden) — it never touches the structural report.
//!
//! The scorer takes embedding vectors (`&[f32]`) directly, never an embedder, so
//! `monomyth-eval` gains no embedding/RAG dependency: a caller embeds prose via
//! `monomyth_knowledge::Knowledge::embed_texts` and hands the vectors here.
//!
//! Gated behind the default-off `semantic` feature so the crate's default surface
//! stays the pure, deterministic core.

use serde::Serialize;

/// Cosine similarity of two equal-length embedding vectors, in `-1.0..=1.0`.
///
/// Accumulated in `f64` for numerical stability regardless of the `f32` inputs.
/// Returns `None` when the vectors differ in length (not comparable), are empty,
/// or either has zero magnitude (cosine is undefined for a zero vector) — so the
/// caller decides how to treat a degenerate pair rather than this silently
/// returning a misleading `0.0`.
///
/// ```
/// use monomyth_eval::cosine_similarity;
///
/// // A vector at 3-4-5 proportions has an integer norm, so its self-similarity is
/// // exactly 1.0 (no floating-point residue).
/// assert_eq!(cosine_similarity(&[3.0_f32, 4.0], &[3.0_f32, 4.0]), Some(1.0));
/// assert_eq!(cosine_similarity(&[1.0, 0.0], &[0.0, 1.0]), Some(0.0));
/// assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), None);
/// ```
#[must_use]
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Option<f64> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let mut dot = 0.0_f64;
    let mut norm_a = 0.0_f64;
    let mut norm_b = 0.0_f64;
    for (&x, &y) in a.iter().zip(b.iter()) {
        let (x, y) = (f64::from(x), f64::from(y));
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    // Norms are sums of squares, so each is `>= 0.0`; `<= 0.0` therefore means an
    // all-zero vector, whose cosine is undefined. (`<=` avoids an exact-equality
    // float comparison.)
    if norm_a <= 0.0 || norm_b <= 0.0 {
        return None;
    }
    // Clamp to the valid cosine range: the `sqrt` division can overshoot `1.0` (or
    // undershoot `-1.0`) by a floating-point epsilon for a vector compared against
    // a scalar multiple of itself, which would otherwise leak an out-of-range score.
    Some((dot / (norm_a.sqrt() * norm_b.sqrt())).clamp(-1.0, 1.0))
}

/// Aggregate prose-similarity across a set of `(predicted, gold)` embedding pairs.
///
/// One pair is one content slot's predicted-vs-gold embedding. Reported scores are
/// `f64` and intentionally *not* quantized here: unlike the structural
/// [`Report`](crate::Report), this axis is never fed to the determinism golden, so
/// it carries full precision for a live baseline to summarize.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct SemanticReport {
    /// The number of comparable pairs. Degenerate pairs (length mismatch or a
    /// zero-magnitude vector) are excluded, so this can be below the input count.
    pub pairs: usize,
    /// Mean cosine similarity across the comparable pairs; `0.0` when there are
    /// none.
    pub mean_cosine: f64,
    /// The lowest cosine similarity — the weakest-grounded slot; `0.0` when there
    /// are no comparable pairs.
    pub min_cosine: f64,
    /// The highest cosine similarity; `0.0` when there are no comparable pairs.
    pub max_cosine: f64,
}

/// Score prose similarity across `(predicted, gold)` embedding pairs.
///
/// Degenerate pairs (length mismatch or a zero vector, per [`cosine_similarity`])
/// are skipped and excluded from [`SemanticReport::pairs`], so an unembeddable slot
/// cannot silently drag the mean toward zero. With no comparable pairs the result
/// is [`SemanticReport::default`] (all-zero, `pairs == 0`).
///
/// ```
/// use monomyth_eval::{score_semantic, SemanticReport};
///
/// let report = score_semantic(&[
///     (vec![1.0_f32, 0.0], vec![1.0_f32, 0.0]), // identical -> 1.0
///     (vec![1.0_f32, 0.0], vec![0.0_f32, 1.0]), // orthogonal -> 0.0
/// ]);
/// assert_eq!(report.pairs, 2);
/// assert_eq!(report.mean_cosine, 0.5);
/// assert_eq!(report.min_cosine, 0.0);
/// assert_eq!(report.max_cosine, 1.0);
/// ```
#[must_use]
pub fn score_semantic(pairs: &[(Vec<f32>, Vec<f32>)]) -> SemanticReport {
    let cosines: Vec<f64> = pairs
        .iter()
        .filter_map(|(predicted, gold)| cosine_similarity(predicted, gold))
        .collect();
    let Some(&first) = cosines.first() else {
        return SemanticReport::default();
    };
    let sum: f64 = cosines.iter().sum();
    let min = cosines.iter().copied().fold(first, f64::min);
    let max = cosines.iter().copied().fold(first, f64::max);
    #[allow(
        clippy::cast_precision_loss,
        reason = "pair count is a small slot count, exactly representable as f64"
    )]
    let mean = sum / cosines.len() as f64;
    SemanticReport {
        pairs: cosines.len(),
        mean_cosine: mean,
        min_cosine: min,
        max_cosine: max,
    }
}

#[cfg(test)]
mod tests {
    use super::{SemanticReport, cosine_similarity, score_semantic};

    /// Cosine is a floating-point ratio, so compare within a tight tolerance rather
    /// than bit-for-bit (a vector's self-similarity is `1.0` only up to `sqrt`
    /// rounding, and clippy's `float_cmp` forbids exact `==` on floats regardless).
    fn assert_close(value: f64, expected: f64) {
        assert!(
            (value - expected).abs() < 1e-9,
            "expected ~{expected}, got {value}",
        );
    }

    /// The cosine of a pair asserted to be comparable, unwrapped for `assert_close`.
    fn cosine(a: &[f32], b: &[f32]) -> f64 {
        cosine_similarity(a, b).expect("the pair is comparable")
    }

    #[test]
    fn cosine_of_identical_vectors_should_be_one() {
        let v = [0.5_f32, -1.5, 2.0];
        assert_close(cosine(&v, &v), 1.0);
    }

    #[test]
    fn cosine_of_orthogonal_vectors_should_be_zero() {
        assert_close(cosine(&[1.0, 0.0], &[0.0, 1.0]), 0.0);
    }

    #[test]
    fn cosine_of_opposite_vectors_should_be_negative_one() {
        assert_close(cosine(&[1.0, 2.0], &[-1.0, -2.0]), -1.0);
    }

    #[test]
    fn cosine_should_be_scale_invariant() {
        assert_close(cosine(&[1.0, 2.0, 3.0], &[2.0, 4.0, 6.0]), 1.0);
    }

    #[test]
    fn cosine_of_mismatched_lengths_should_be_none() {
        assert_eq!(cosine_similarity(&[1.0, 2.0], &[1.0]), None);
    }

    #[test]
    fn cosine_of_empty_vectors_should_be_none() {
        assert_eq!(cosine_similarity(&[], &[]), None);
    }

    #[test]
    fn cosine_of_a_zero_vector_should_be_none() {
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), None);
    }

    #[test]
    fn score_semantic_should_aggregate_mean_min_max() {
        let report = score_semantic(&[
            (vec![1.0, 0.0], vec![1.0, 0.0]),  // 1.0
            (vec![1.0, 0.0], vec![0.0, 1.0]),  // 0.0
            (vec![1.0, 0.0], vec![-1.0, 0.0]), // -1.0
        ]);
        assert_eq!(report.pairs, 3);
        assert_close(report.mean_cosine, 0.0);
        assert_close(report.min_cosine, -1.0);
        assert_close(report.max_cosine, 1.0);
    }

    #[test]
    fn score_semantic_should_skip_degenerate_pairs() {
        let report = score_semantic(&[
            (vec![1.0, 0.0], vec![1.0, 0.0]), // comparable -> 1.0
            (vec![1.0, 0.0], vec![0.0, 0.0]), // zero vector -> skipped
            (vec![1.0], vec![1.0, 2.0]),      // length mismatch -> skipped
        ]);
        assert_eq!(report.pairs, 1, "only the one comparable pair is counted");
        assert_close(report.mean_cosine, 1.0);
    }

    #[test]
    fn score_semantic_with_no_comparable_pairs_should_be_default() {
        let report = score_semantic(&[(vec![0.0, 0.0], vec![1.0, 1.0])]);
        assert_eq!(report, SemanticReport::default());
        assert_eq!(report.pairs, 0);
    }
}
