//! The configuration schema: the on-disk file shape ([`MonomythConfigFile`]) and
//! the resolved, layer-tagged shape ([`MonomythConfig`]).
//!
//! This is the config spine (ADR-0015). Each knob appears three times: an
//! `Option`-typed field on the file struct, a `Layered<T>` field on the resolved
//! struct, and one line in [`MonomythConfig::apply_file`]. It currently carries
//! `generation.fork_chance_permille` and the `[models]` table (per-task model
//! routing); later slices add `[retrieval]`, `[synthesis]`, `[paths]`, and
//! `[knowledge]` the same way.

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

/// The system-default `provider/model` routing string for the per-slot content
/// pass. Gemini Flash is the cheaper, faster tier suited to content fill.
///
/// This is the single authoring point for the content model id: it was moved here
/// out of the CLI so no model string is hardcoded in code or tests.
const DEFAULT_CONTENT_MODEL: &str = "gemini/gemini-3.5-flash";

/// The system-default `provider/model` routing string for the law-synthesis
/// distillation pass. A PRO tier, since distillation quality outweighs latency.
const DEFAULT_SYNTHESIS_MODEL: &str = "gemini/gemini-3.1-pro-preview";

/// A per-task model role: which configured model a caller resolves via
/// [`MonomythConfig::model_for`].
///
/// Splitting model selection by role is what lets a cheap tier drive content fill
/// while a quality tier drives synthesis, from one config table. New roles (a
/// dedicated judge, an extractor) are added here as their call sites migrate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelRole {
    /// The per-slot content-fill pass (`gen --fill`).
    Content,
    /// The law-synthesis distillation pass (`synthesize law`).
    Synthesis,
}

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
    /// The `[models]` table.
    pub models: ModelsSection,
}

/// The `[generation]` table as it appears on disk.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GenerationSection {
    /// Probability, in permille (`0..=1000`), that a run of optional narrative
    /// stages forks into a player choice. Absent → the system default.
    pub fork_chance_permille: Option<u16>,
}

/// The `[models]` table as it appears on disk. Each value is a `provider/model`
/// routing string (e.g. `"gemini/gemini-3.5-flash"`); the API key is never here —
/// it stays in the environment (`.env`).
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ModelsSection {
    /// Overrides the content-fill model when present.
    pub content: Option<String>,
    /// Overrides the law-synthesis model when present.
    pub synthesis: Option<String>,
}

/// The fully-resolved configuration: every value tagged with the [`Layered`] source
/// that set it.
#[derive(Clone, Debug)]
pub struct MonomythConfig {
    /// Resolved narrative-generation knobs.
    pub generation: GenerationSettings,
    /// Resolved per-task model routing.
    pub models: ModelsSettings,
}

/// Resolved narrative-generation knobs.
#[derive(Clone, Debug)]
pub struct GenerationSettings {
    /// Resolved optional-stage fork probability, in permille (`0..=1000`).
    pub fork_chance_permille: Layered<u16>,
}

/// Resolved per-task model routing.
#[derive(Clone, Debug)]
pub struct ModelsSettings {
    /// Resolved content-fill model routing string.
    pub content: Layered<String>,
    /// Resolved law-synthesis model routing string.
    pub synthesis: Layered<String>,
}

impl Default for MonomythConfig {
    fn default() -> Self {
        Self {
            generation: GenerationSettings {
                fork_chance_permille: Layered::system_default(DEFAULT_FORK_CHANCE_PERMILLE),
            },
            models: ModelsSettings {
                content: Layered::system_default(DEFAULT_CONTENT_MODEL.to_owned()),
                synthesis: Layered::system_default(DEFAULT_SYNTHESIS_MODEL.to_owned()),
            },
        }
    }
}

impl MonomythConfig {
    /// The resolved `provider/model` routing string for `role`.
    #[must_use]
    pub fn model_for(&self, role: ModelRole) -> &str {
        match role {
            ModelRole::Content => self.models.content.get(),
            ModelRole::Synthesis => self.models.synthesis.get(),
        }
    }

    /// Fold one parsed configuration file into this resolved config at layer
    /// `from`. Each field's [`Layered::override_with`] enforces precedence, so the
    /// call order of layers does not matter — only their [`LayerSource`] rank.
    pub(crate) fn apply_file(&mut self, file: &MonomythConfigFile, from: LayerSource) {
        self.generation
            .fork_chance_permille
            .override_with(file.generation.fork_chance_permille, from);
        self.models
            .content
            .override_with(file.models.content.clone(), from);
        self.models
            .synthesis
            .override_with(file.models.synthesis.clone(), from);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_CONTENT_MODEL, DEFAULT_FORK_CHANCE_PERMILLE, DEFAULT_SYNTHESIS_MODEL, ModelRole,
        MonomythConfig, MonomythConfigFile,
    };
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
    fn default_resolves_the_system_default_models() {
        let config = MonomythConfig::default();
        assert_eq!(config.model_for(ModelRole::Content), DEFAULT_CONTENT_MODEL);
        assert_eq!(
            config.model_for(ModelRole::Synthesis),
            DEFAULT_SYNTHESIS_MODEL
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
    fn a_parsed_file_overrides_a_single_model_leaving_the_other_default() {
        let file: MonomythConfigFile =
            toml::from_str("[models]\ncontent = \"anthropic/claude-haiku-4-5\"\n")
                .expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        assert_eq!(
            config.model_for(ModelRole::Content),
            "anthropic/claude-haiku-4-5"
        );
        assert_eq!(
            config.models.content.source(),
            LayerSource::ProjectOverride,
            "the overridden model advances to the file layer"
        );
        assert_eq!(
            config.model_for(ModelRole::Synthesis),
            DEFAULT_SYNTHESIS_MODEL,
            "an untouched model keeps its system default"
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
