//! The configuration schema: the on-disk file shape ([`MonomythConfigFile`]) and
//! the resolved, layer-tagged shape ([`MonomythConfig`]).
//!
//! This is the first slice of the config spine (ADR-0015): it carries a single
//! knob, `generation.fork_chance_permille`, end to end. Later slices add the
//! `[models]`, `[retrieval]`, `[synthesis]`, `[paths]`, and `[knowledge]` sections
//! the same way — an `Option`-typed field on the file struct, a `Layered<T>` field
//! on the resolved struct, and one line in [`MonomythConfig::apply_file`].

use serde::Deserialize;

use crate::layered::{LayerSource, Layered};

/// The system default for `generation.fork_chance_permille`.
///
/// This intentionally duplicates `monomyth-gen`'s
/// `DEFAULT_FORK_CHANCE_PERMILLE` for the duration of the migration: the two must
/// agree, and that agreement is guarded by `monomyth-cli`'s determinism test
/// (which resolves this default, feeds it to the generator, and asserts the output
/// is byte-identical to `Generator::with_default_passes()`). `monomyth-config` does
/// not depend on `monomyth-gen`, so the value cannot simply be imported.
const DEFAULT_FORK_CHANCE_PERMILLE: u16 = 500;

/// The on-disk configuration file shape: every field optional, so an omitted key
/// leaves the lower layer untouched.
///
/// `deny_unknown_fields` makes a misspelled key a hard parse error rather than a
/// silently-ignored no-op; `default` lets a whole section be omitted.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MonomythConfigFile {
    /// The `[generation]` table.
    pub generation: GenerationSection,
}

/// The `[generation]` table as it appears on disk.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GenerationSection {
    /// Probability, in permille (`0..=1000`), that a run of optional narrative
    /// stages forks into a player choice. Absent → the system default.
    pub fork_chance_permille: Option<u16>,
}

/// The fully-resolved configuration: every value tagged with the [`Layered`] source
/// that set it.
#[derive(Clone, Debug)]
pub struct MonomythConfig {
    /// Resolved narrative-generation knobs.
    pub generation: GenerationSettings,
}

/// Resolved narrative-generation knobs.
#[derive(Clone, Debug)]
pub struct GenerationSettings {
    /// Resolved optional-stage fork probability, in permille (`0..=1000`).
    pub fork_chance_permille: Layered<u16>,
}

impl Default for MonomythConfig {
    fn default() -> Self {
        Self {
            generation: GenerationSettings {
                fork_chance_permille: Layered::system_default(DEFAULT_FORK_CHANCE_PERMILLE),
            },
        }
    }
}

impl MonomythConfig {
    /// Fold one parsed configuration file into this resolved config at layer
    /// `from`. Each field's [`Layered::override_with`] enforces precedence, so the
    /// call order of layers does not matter — only their [`LayerSource`] rank.
    pub(crate) fn apply_file(&mut self, file: &MonomythConfigFile, from: LayerSource) {
        self.generation
            .fork_chance_permille
            .override_with(file.generation.fork_chance_permille, from);
    }
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_FORK_CHANCE_PERMILLE, MonomythConfig, MonomythConfigFile};
    use crate::layered::LayerSource;

    #[test]
    fn default_resolves_the_system_default_fork_chance() {
        let config = MonomythConfig::default();
        assert_eq!(
            *config.generation.fork_chance_permille.get(),
            DEFAULT_FORK_CHANCE_PERMILLE
        );
        assert_eq!(
            config.generation.fork_chance_permille.source(),
            LayerSource::SystemDefault
        );
    }

    #[test]
    fn a_parsed_file_overrides_the_default_at_its_layer() {
        let file: MonomythConfigFile =
            toml::from_str("[generation]\nfork_chance_permille = 250\n").expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        assert_eq!(*config.generation.fork_chance_permille.get(), 250);
        assert_eq!(
            config.generation.fork_chance_permille.source(),
            LayerSource::ProjectOverride
        );
    }

    #[test]
    fn an_empty_file_leaves_the_default_intact() {
        let file: MonomythConfigFile = toml::from_str("").expect("empty toml is valid");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        assert_eq!(
            *config.generation.fork_chance_permille.get(),
            DEFAULT_FORK_CHANCE_PERMILLE
        );
        assert_eq!(
            config.generation.fork_chance_permille.source(),
            LayerSource::SystemDefault,
            "an absent key must not advance the layer",
        );
    }

    #[test]
    fn an_unknown_key_is_a_hard_parse_error() {
        let result: Result<MonomythConfigFile, _> =
            toml::from_str("[generation]\nfork_chance_permile = 250\n");
        assert!(
            result.is_err(),
            "deny_unknown_fields must reject a misspelled key"
        );
    }
}
