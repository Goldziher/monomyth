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

use crate::passes::{BeatConfig, CastConfig, ItemsConfig, MapConfig, NarrativeConfig};

/// Resolved knobs for the procedural generation passes.
///
/// Carries the macro fork probability plus the beat, map, item, and cast
/// bounds — every procedural pass's tunable knobs, flattened into one scalar
/// bag for the CLI to hand the generator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GenerationConfig {
    /// Probability, in permille (`0..=1000`), that a run of optional narrative
    /// stages forks into a player choice. Feeds
    /// [`NarrativeConfig::fork_chance_permille`].
    pub fork_chance_permille: u16,
    /// The fewest beats [`BeatPass`](crate::BeatPass) splices onto each stage
    /// node's spine. Feeds [`BeatConfig::beats_per_stage_min`].
    pub beats_per_stage_min: usize,
    /// The most beats [`BeatPass`](crate::BeatPass) splices onto each stage
    /// node's spine. Feeds [`BeatConfig::beats_per_stage_max`].
    pub beats_per_stage_max: usize,
    /// The fewest rooms a generated world may contain. Feeds
    /// [`MapConfig::rooms_min`].
    pub rooms_min: usize,
    /// The most rooms a generated world may contain. Feeds
    /// [`MapConfig::rooms_max`].
    pub rooms_max: usize,
    /// The fewest items scattered across the world. Feeds
    /// [`ItemsConfig::items_min`].
    pub items_min: usize,
    /// The most items scattered across the world. Feeds
    /// [`ItemsConfig::items_max`].
    pub items_max: usize,
    /// The most supporting roles added on top of the base cast. Feeds
    /// [`CastConfig::max_extra_cast`].
    pub max_extra_cast: usize,
}

impl Default for GenerationConfig {
    fn default() -> Self {
        Self {
            fork_chance_permille: NarrativeConfig::default().fork_chance_permille,
            beats_per_stage_min: BeatConfig::default().beats_per_stage_min,
            beats_per_stage_max: BeatConfig::default().beats_per_stage_max,
            rooms_min: MapConfig::default().rooms_min,
            rooms_max: MapConfig::default().rooms_max,
            items_min: ItemsConfig::default().items_min,
            items_max: ItemsConfig::default().items_max,
            max_extra_cast: CastConfig::default().max_extra_cast,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::GenerationConfig;
    use crate::passes::{BeatConfig, CastConfig, ItemsConfig, MapConfig};
    use crate::{MAX_ROOMS, MIN_ROOMS};

    #[test]
    fn default_reproduces_every_pass_configs_own_default() {
        let config = GenerationConfig::default();
        let beat = BeatConfig::default();
        let map = MapConfig::default();
        let items = ItemsConfig::default();
        let cast = CastConfig::default();

        assert_eq!(config.beats_per_stage_min, beat.beats_per_stage_min);
        assert_eq!(config.beats_per_stage_max, beat.beats_per_stage_max);
        assert_eq!(config.rooms_min, map.rooms_min);
        assert_eq!(config.rooms_max, map.rooms_max);
        assert_eq!(config.rooms_min, MIN_ROOMS);
        assert_eq!(config.rooms_max, MAX_ROOMS);
        assert_eq!(config.items_min, items.items_min);
        assert_eq!(config.items_max, items.items_max);
        assert_eq!(config.max_extra_cast, cast.max_extra_cast);
    }
}
