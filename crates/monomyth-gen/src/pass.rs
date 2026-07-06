//! The [`ProceduralPass`] trait and the deterministic draw helpers passes share.
//!
//! A pass mutates a [`World`] in place, drawing only from the per-pass
//! [`ChaCha8Rng`] it is handed. It never touches [`World::rng`], which is the
//! quarantined play-time stream. The draw helpers here reduce a raw `u64` from the
//! stream into a bounded index or flag using only `TryFrom` conversions, so they
//! stay deterministic and free of lossy `as` casts.

use monomyth_core::World;
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
    // `high - low + 1` is the count of values in the inclusive range; it is at
    // least 1, so the modulo is well defined.
    let span = high - low + 1;
    let span = u64::try_from(span).expect("usize span fits in u64 on supported platforms");
    // The modulo result is `< span <= usize::MAX`, so it always fits back in usize.
    let offset = usize::try_from(rng.next_u64() % span).expect("modulo result fits in usize");
    low + offset
}

/// Draw a `bool` from the low bit of the next stream word.
pub(crate) fn draw_bool(rng: &mut ChaCha8Rng) -> bool {
    rng.next_u64() & 1 == 1
}
