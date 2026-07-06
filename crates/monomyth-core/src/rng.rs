//! Deterministic, serializable random state for the play engine.
//!
//! A play session must replay exactly from `(seed, action-log)`, so any
//! randomness the engine consumes has to be reproducible across a
//! save/load boundary. [`RngState`] stores only a `seed` and a `word_pos`
//! (the stream offset in 32-bit words) rather than a live RNG. This deliberately
//! avoids depending on `rand_chacha`'s internal serde layout: the concrete
//! [`ChaCha8Rng`] is reconstructed on demand from `(seed, word_pos)`, drawn from,
//! and its advanced position written back. Two [`RngState`]s with equal fields
//! therefore produce identical draw sequences, and a round-tripped state
//! continues exactly where it left off.
//!
//! This is the *procedural* stream. The content (LLM) phase is quarantined and
//! never touches it.

use rand::{RngCore, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

/// Reproducible position in a seeded `ChaCha8` stream.
///
/// Serializes to just its two fields, so it round-trips byte-stably and never
/// depends on the RNG crate's private state encoding.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RngState {
    /// The stream seed; fixes the entire sequence.
    seed: u64,
    /// Offset into the stream, in 32-bit words, advanced after every draw.
    word_pos: u128,
}

impl RngState {
    /// A fresh state at the start of the stream for `seed`.
    ///
    /// ```
    /// use monomyth_core::RngState;
    ///
    /// let mut a = RngState::new(42);
    /// let mut b = RngState::new(42);
    /// assert_eq!(a.next_u64(), b.next_u64());
    /// ```
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { seed, word_pos: 0 }
    }

    /// The seed fixing this stream.
    #[must_use]
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// The current offset into the stream, in 32-bit words.
    #[must_use]
    pub const fn word_pos(&self) -> u128 {
        self.word_pos
    }

    /// Reconstruct the RNG positioned at the current offset.
    fn positioned(&self) -> ChaCha8Rng {
        let mut rng = ChaCha8Rng::seed_from_u64(self.seed);
        rng.set_word_pos(self.word_pos);
        rng
    }

    /// Draw the next `u32`, advancing and persisting the stream position.
    pub fn next_u32(&mut self) -> u32 {
        let mut rng = self.positioned();
        let value = rng.next_u32();
        self.word_pos = rng.get_word_pos();
        value
    }

    /// Draw the next `u64`, advancing and persisting the stream position.
    ///
    /// ```
    /// use monomyth_core::RngState;
    ///
    /// let mut state = RngState::new(7);
    /// let _first = state.next_u64();
    /// // A round-trip through serde continues the same sequence.
    /// let encoded = serde_json::to_string(&state)?;
    /// let mut restored: RngState = serde_json::from_str(&encoded)?;
    /// let _second = state.next_u64();
    /// assert_eq!(restored.next_u64(), _second);
    /// assert_eq!(state.word_pos(), 4, "two u64 draws consume four 32-bit words");
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    pub fn next_u64(&mut self) -> u64 {
        let mut rng = self.positioned();
        let value = rng.next_u64();
        self.word_pos = rng.get_word_pos();
        value
    }
}
