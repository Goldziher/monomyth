//! The [`ProceduralPass`] trait and the deterministic draw helpers passes share.
//!
//! A pass mutates a [`World`] in place, drawing only from the per-pass
//! [`ChaCha8Rng`] it is handed. It never touches [`World::rng`], which is the
//! quarantined play-time stream. The draw helpers here reduce a raw `u64` from the
//! stream into a bounded index or flag using only `TryFrom` conversions, so they
//! stay deterministic and free of lossy `as` casts.

use monomyth_core::{Weight, World};
use rand::RngCore;
use rand_chacha::ChaCha8Rng;

use crate::error::GenError;

/// One ordered stage of procedural structure generation.
///
/// Passes are boxed as trait objects in a [`Generator`](crate::Generator)
/// pipeline, so the trait requires [`Debug`](std::fmt::Debug) (for the workspace
/// `missing_debug_implementations` lint) and [`Send`] + [`Sync`] (so a pipeline
/// can be shared across threads).
pub trait ProceduralPass: std::fmt::Debug + Send + Sync {
    /// A stable, human-readable name used in error messages and diagnostics.
    fn name(&self) -> &'static str;

    /// Apply this pass's structural changes to `world`, drawing from `rng`.
    ///
    /// All content slots the pass creates are left
    /// [`Empty`](monomyth_core::Content::Empty); only the later content phase
    /// fills them.
    ///
    /// # Errors
    ///
    /// Returns a [`GenError`] if the pass's structural preconditions are not met
    /// (for example, a cast pass run before any locations exist).
    fn apply(&self, world: &mut World, rng: &mut ChaCha8Rng) -> Result<(), GenError>;
}

/// Draw a `usize` uniformly from the inclusive range `[low, high]`.
///
/// Deterministic given the stream position. The modulo reduction can introduce a
/// negligible bias for ranges that do not divide `2^64`; this is acceptable for
/// procedural layout and keeps the reduction dependency-stable (it does not rely
/// on `rand`'s range-sampling internals, which the golden-hash test would pin).
pub(crate) fn draw_range_inclusive(rng: &mut ChaCha8Rng, low: usize, high: usize) -> usize {
    debug_assert!(low <= high, "draw range must be non-empty and ordered");
    let span = high - low + 1;
    let span = u64::try_from(span).expect("usize span fits in u64 on supported platforms");
    let offset = usize::try_from(rng.next_u64() % span).expect("modulo result fits in usize");
    low + offset
}

/// Draw a `bool` from the low bit of the next stream word.
pub(crate) fn draw_bool(rng: &mut ChaCha8Rng) -> bool {
    rng.next_u64() & 1 == 1
}

/// Draw a `bool` that is `true` with probability `permille` in 1000.
///
/// Integer permille keeps the decision float-free and stream-stable, matching the
/// modulo reduction the other helpers use. `permille >= 1000` is always `true`;
/// `0` is always `false`. Consumes exactly one stream word regardless of outcome,
/// so a caller's stream position never depends on the result.
pub(crate) fn draw_chance(rng: &mut ChaCha8Rng, permille: u16) -> bool {
    rng.next_u64() % 1000 < u64::from(permille)
}

/// Draw an index into `weights` proportional to each entry's relative share.
///
/// Consumes exactly one RNG stream word (`ChaCha8Rng::next_u64` via `RngCore`)
/// unconditionally — including when `weights` is empty or all-zero — so a
/// caller's stream position never depends on how many weights it was passed or
/// their values. This is why the draw happens before the zero-total check rather
/// than being short-circuited by it. Weights are relative shares — they need not
/// sum to 1000 (or any particular total); the total is normalized internally.
/// Returns `None` if `weights` is empty or all weights are zero (nothing to draw).
pub(crate) fn draw_weighted_index(rng: &mut ChaCha8Rng, weights: &[Weight]) -> Option<usize> {
    let r = rng.next_u64();
    let total: u64 = weights
        .iter()
        .map(|weight| u64::from(weight.permille()))
        .sum();
    if total == 0 {
        return None;
    }
    let r = r % total;
    let mut cumulative: u64 = 0;
    for (index, weight) in weights.iter().enumerate() {
        cumulative += u64::from(weight.permille());
        if cumulative > r {
            return Some(index);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use rand::SeedableRng;

    use super::*;

    /// `draw_weighted_index` must consume exactly one stream word, so a caller's
    /// subsequent draws are unaffected by how many weights it was passed or what
    /// index was chosen. Verified by comparing a stream that goes through
    /// `draw_weighted_index` against a stream that consumes one raw `next_u64`.
    #[test]
    fn should_consume_exactly_one_stream_word() {
        let mut rng_a = ChaCha8Rng::seed_from_u64(9001);
        let mut rng_b = ChaCha8Rng::seed_from_u64(9001);

        draw_weighted_index(&mut rng_a, &[Weight::new(300), Weight::new(700)]);
        rng_b.next_u64();

        assert_eq!(
            rng_a.next_u64(),
            rng_b.next_u64(),
            "draw_weighted_index must advance the stream by exactly one word",
        );
    }

    /// The single-stream-word guarantee must also hold on the `None` early-return
    /// path (empty or all-zero weights): the draw happens before the zero-total
    /// check, so a caller falling back on `None` still lands on the same stream
    /// position as a caller that drew a real index.
    #[test]
    fn should_consume_exactly_one_stream_word_even_when_weights_are_all_zero() {
        let mut rng_a = ChaCha8Rng::seed_from_u64(9001);
        let mut rng_b = ChaCha8Rng::seed_from_u64(9001);

        let result = draw_weighted_index(&mut rng_a, &[Weight::ZERO, Weight::ZERO]);
        rng_b.next_u64();

        assert_eq!(result, None, "an all-zero weight pool has nothing to draw");
        assert_eq!(
            rng_a.next_u64(),
            rng_b.next_u64(),
            "draw_weighted_index must advance the stream by exactly one word \
             even on the all-zero-weight None path",
        );
    }

    /// A chi-square goodness-of-fit test: with K equal, nonzero weights, repeated
    /// draws must be statistically indistinguishable from uniform selection.
    ///
    /// Fixed seed 12345, N = 10,000 draws, K = 6 equally weighted candidates. The
    /// chi-square statistic `sum((observed - expected)^2 / expected)` is compared
    /// against the critical value for `K - 1 = 5` degrees of freedom at
    /// alpha = 0.001, which is 20.515 (chi-square distribution table, df=5,
    /// alpha=0.001). alpha=0.001 is strict enough that a true-uniform generator
    /// practically never exceeds this threshold by chance for a fixed, pinned
    /// seed — avoiding CI flakiness — while still being a meaningful bias
    /// detector for a real skew in the draw.
    #[test]
    fn uniform_weights_select_without_bias() {
        const SEED: u64 = 12345;
        const CANDIDATE_COUNT: usize = 6;
        const DRAW_COUNT: u32 = 10_000;
        const CHI_SQUARE_CRITICAL_VALUE_DF5_ALPHA0001: f64 = 20.515;

        let mut rng = ChaCha8Rng::seed_from_u64(SEED);
        let weights = vec![Weight::new(1000); CANDIDATE_COUNT];
        let mut observed = [0u32; CANDIDATE_COUNT];

        for _ in 0..DRAW_COUNT {
            let index = draw_weighted_index(&mut rng, &weights)
                .expect("nonzero equal weights always yield a draw");
            observed[index] += 1;
        }

        let candidate_count = u32::try_from(CANDIDATE_COUNT).expect("candidate count fits u32");
        let expected = f64::from(DRAW_COUNT) / f64::from(candidate_count);
        let chi_square: f64 = observed
            .iter()
            .map(|&count| {
                let diff = f64::from(count) - expected;
                diff * diff / expected
            })
            .sum();

        assert!(
            chi_square < CHI_SQUARE_CRITICAL_VALUE_DF5_ALPHA0001,
            "chi-square statistic {chi_square} exceeds the df=5, alpha=0.001 critical value \
             {CHI_SQUARE_CRITICAL_VALUE_DF5_ALPHA0001}; uniform weights produced biased draws: {observed:?}",
        );
    }
}
