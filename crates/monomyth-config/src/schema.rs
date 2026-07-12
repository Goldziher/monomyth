//! The configuration schema: the on-disk file shape ([`MonomythConfigFile`]) and
//! the resolved, layer-tagged shape ([`MonomythConfig`]).
//!
//! This is the config spine (ADR-0015). Each knob appears three times: an
//! `Option`-typed field on the file struct, a `Layered<T>` field on the resolved
//! struct, and one line in [`MonomythConfig::apply_file`]. It currently carries
//! the `[generation]` table (the macro fork probability plus the beat, map,
//! item, and cast procedural bounds) and the `[models]` table (per-task model
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

/// The system default for `generation.beats_per_stage_min`.
///
/// Duplicates `monomyth-gen`'s `BeatConfig` default; see
/// [`DEFAULT_FORK_CHANCE_PERMILLE`] for why the duplication is deliberate.
const DEFAULT_BEATS_PER_STAGE_MIN: usize = 1;
/// The system default for `generation.beats_per_stage_max`.
const DEFAULT_BEATS_PER_STAGE_MAX: usize = 3;
/// The system default for `generation.rooms_min`.
const DEFAULT_ROOMS_MIN: usize = 5;
/// The system default for `generation.rooms_max`.
const DEFAULT_ROOMS_MAX: usize = 9;
/// The system default for `generation.items_min`.
const DEFAULT_ITEMS_MIN: usize = 2;
/// The system default for `generation.items_max`.
const DEFAULT_ITEMS_MAX: usize = 6;
/// The system default for `generation.max_extra_cast`.
const DEFAULT_MAX_EXTRA_CAST: usize = 3;

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
    /// The fewest beats spliced onto each stage node's spine. Absent → the
    /// system default.
    pub beats_per_stage_min: Option<usize>,
    /// The most beats spliced onto each stage node's spine. Absent → the
    /// system default.
    pub beats_per_stage_max: Option<usize>,
    /// The fewest rooms a generated world may contain. Absent → the system
    /// default.
    pub rooms_min: Option<usize>,
    /// The most rooms a generated world may contain. Absent → the system
    /// default.
    pub rooms_max: Option<usize>,
    /// The fewest items scattered across the world. Absent → the system
    /// default.
    pub items_min: Option<usize>,
    /// The most items scattered across the world. Absent → the system
    /// default.
    pub items_max: Option<usize>,
    /// The most supporting roles added on top of the base cast. Absent → the
    /// system default.
    pub max_extra_cast: Option<usize>,
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
    /// Resolved fewest beats spliced onto each stage node's spine.
    pub beats_per_stage_min: Layered<usize>,
    /// Resolved most beats spliced onto each stage node's spine.
    pub beats_per_stage_max: Layered<usize>,
    /// Resolved fewest rooms a generated world may contain.
    pub rooms_min: Layered<usize>,
    /// Resolved most rooms a generated world may contain.
    pub rooms_max: Layered<usize>,
    /// Resolved fewest items scattered across the world.
    pub items_min: Layered<usize>,
    /// Resolved most items scattered across the world.
    pub items_max: Layered<usize>,
    /// Resolved most supporting roles added on top of the base cast.
    pub max_extra_cast: Layered<usize>,
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
                beats_per_stage_min: Layered::system_default(DEFAULT_BEATS_PER_STAGE_MIN),
                beats_per_stage_max: Layered::system_default(DEFAULT_BEATS_PER_STAGE_MAX),
                rooms_min: Layered::system_default(DEFAULT_ROOMS_MIN),
                rooms_max: Layered::system_default(DEFAULT_ROOMS_MAX),
                items_min: Layered::system_default(DEFAULT_ITEMS_MIN),
                items_max: Layered::system_default(DEFAULT_ITEMS_MAX),
                max_extra_cast: Layered::system_default(DEFAULT_MAX_EXTRA_CAST),
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
        self.generation
            .beats_per_stage_min
            .override_with(file.generation.beats_per_stage_min, from);
        self.generation
            .beats_per_stage_max
            .override_with(file.generation.beats_per_stage_max, from);
        self.generation
            .rooms_min
            .override_with(file.generation.rooms_min, from);
        self.generation
            .rooms_max
            .override_with(file.generation.rooms_max, from);
        self.generation
            .items_min
            .override_with(file.generation.items_min, from);
        self.generation
            .items_max
            .override_with(file.generation.items_max, from);
        self.generation
            .max_extra_cast
            .override_with(file.generation.max_extra_cast, from);
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
        DEFAULT_BEATS_PER_STAGE_MAX, DEFAULT_BEATS_PER_STAGE_MIN, DEFAULT_CONTENT_MODEL,
        DEFAULT_FORK_CHANCE_PERMILLE, DEFAULT_ITEMS_MAX, DEFAULT_ITEMS_MIN, DEFAULT_MAX_EXTRA_CAST,
        DEFAULT_ROOMS_MAX, DEFAULT_ROOMS_MIN, DEFAULT_SYNTHESIS_MODEL, ModelRole, MonomythConfig,
        MonomythConfigFile,
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
    fn default_resolves_the_system_default_procedural_bounds() {
        let config = MonomythConfig::default();
        assert_eq!(
            *config.generation.beats_per_stage_min.get(),
            DEFAULT_BEATS_PER_STAGE_MIN
        );
        assert_eq!(
            *config.generation.beats_per_stage_max.get(),
            DEFAULT_BEATS_PER_STAGE_MAX
        );
        assert_eq!(*config.generation.rooms_min.get(), DEFAULT_ROOMS_MIN);
        assert_eq!(*config.generation.rooms_max.get(), DEFAULT_ROOMS_MAX);
        assert_eq!(*config.generation.items_min.get(), DEFAULT_ITEMS_MIN);
        assert_eq!(*config.generation.items_max.get(), DEFAULT_ITEMS_MAX);
        assert_eq!(
            *config.generation.max_extra_cast.get(),
            DEFAULT_MAX_EXTRA_CAST
        );
    }

    #[test]
    fn a_parsed_file_overrides_every_procedural_bound_at_its_layer() {
        let file: MonomythConfigFile = toml::from_str(
            "[generation]\n\
             beats_per_stage_min = 2\n\
             beats_per_stage_max = 4\n\
             rooms_min = 6\n\
             rooms_max = 10\n\
             items_min = 3\n\
             items_max = 7\n\
             max_extra_cast = 5\n",
        )
        .expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);

        assert_eq!(*config.generation.beats_per_stage_min.get(), 2);
        assert_eq!(*config.generation.beats_per_stage_max.get(), 4);
        assert_eq!(*config.generation.rooms_min.get(), 6);
        assert_eq!(*config.generation.rooms_max.get(), 10);
        assert_eq!(*config.generation.items_min.get(), 3);
        assert_eq!(*config.generation.items_max.get(), 7);
        assert_eq!(*config.generation.max_extra_cast.get(), 5);
        assert_eq!(
            config.generation.rooms_min.source(),
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
