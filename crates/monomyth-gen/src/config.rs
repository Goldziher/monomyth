//! [`GenerationConfig`]: the resolved, plain-scalar knobs for the procedural
//! pipeline.
//!
//! This is deliberately a bag of already-resolved scalars, *not* the layered
//! `monomyth-config` types: `monomyth-gen` stays free of any dependency on the
//! config crate. The CLI resolves the layered configuration and hands the
//! generator this flat struct via [`Generator::with_config`](crate::Generator::with_config).
//!
//! [`GenerationConfig::default`] reproduces the passes' own defaults exactly, so
//! `Generator::with_config(&GenerationConfig::default())` is byte-identical to
//! [`Generator::with_default_passes`](crate::Generator::with_default_passes) — the
//! guarantee that keeps an unconfigured run from drifting the determinism golden.

use crate::passes::NarrativeConfig;

/// Resolved knobs for the procedural generation passes.
///
/// v1 carries only the macro fork probability; later slices grow it to cover the
/// beat, map, item, and cast bounds as those constants migrate to configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenerationConfig {
    /// Probability, in permille (`0..=1000`), that a run of optional narrative
    /// stages forks into a player choice. Feeds
    /// [`NarrativeConfig::fork_chance_permille`].
    pub fork_chance_permille: u16,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            fork_chance_permille: NarrativeConfig::default().fork_chance_permille,
        }
    }
}
