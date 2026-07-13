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

use crate::error::ConfigError;
use crate::layered::{LayerSource, Layered};

/// The inclusive upper bound for any permille-valued knob.
const PERMILLE_MAX: u16 = 1000;

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

/// The system default for `synthesis.max_iterations`.
///
/// Duplicates `monomyth-synthesis`'s `LoopConfig` defaults for the duration of the
/// migration; the two must agree, guarded by `monomyth-cli`'s test asserting the
/// resolved-default synthesis config equals `LoopConfig::default()`.
const DEFAULT_MAX_ITERATIONS: u32 = 3;
/// The system default for `synthesis.per_query_top_k`.
const DEFAULT_PER_QUERY_TOP_K: u32 = 8;
/// The system default for `synthesis.max_grounding`.
const DEFAULT_MAX_GROUNDING: usize = 24;

/// The system-default `provider/model` routing string for the per-slot content
/// pass. Gemini Flash is the cheaper, faster tier suited to content fill.
///
/// This is the single authoring point for the content model id: it was moved here
/// out of the CLI so no model string is hardcoded in code or tests.
const DEFAULT_CONTENT_MODEL: &str = "gemini/gemini-3.5-flash";

/// The system-default `provider/model` routing string for the law-synthesis
/// distillation pass. A PRO tier, since distillation quality outweighs latency.
const DEFAULT_SYNTHESIS_MODEL: &str = "gemini/gemini-3.1-pro-preview";

/// The system-default `provider/model` routing string for the long-form
/// composition pipeline (`compose`). Long-form prose generation uses the
/// cheaper/faster flash tier by default; a quality tier can be selected via
/// `[models].compose` or `--model`.
///
/// This is the single authoring point for the compose model id: no model
/// string is hardcoded anywhere else in code or tests.
const DEFAULT_COMPOSE_MODEL: &str = "gemini/gemini-3.5-flash";

/// The system default for `compose.max_turns`.
///
/// Duplicates `monomyth-compose`'s `ComposeSettings` default for the duration of
/// the migration; the two must agree, guarded by `monomyth-cli`'s test asserting
/// the resolved-default compose config equals `ComposeSettings::default()`.
const DEFAULT_COMPOSE_MAX_TURNS: usize = 5;
/// The system default for `compose.grounding_top_k`.
const DEFAULT_COMPOSE_GROUNDING_TOP_K: u32 = 4;
/// The system default for `compose.revise_threshold`.
const DEFAULT_COMPOSE_REVISE_THRESHOLD: f64 = 0.6;
/// The system default for `compose.max_revise_iterations`.
const DEFAULT_COMPOSE_MAX_REVISE_ITERATIONS: usize = 2;

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
    /// The long-form composition pipeline (`compose`).
    Compose,
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
    /// The `[synthesis]` table.
    pub synthesis: SynthesisSection,
    /// The `[compose]` table.
    pub compose: ComposeSection,
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
    /// Overrides the long-form composition model when present.
    pub compose: Option<String>,
}

/// The `[synthesis]` table as it appears on disk: the law-synthesis judge loop's
/// integer tuning knobs. The f64 passing-bar thresholds stay as
/// `monomyth-synthesis::LoopConfig` internals for now.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct SynthesisSection {
    /// Max judge/refine iterations before the loop stops. Absent → the system
    /// default.
    pub max_iterations: Option<u32>,
    /// Reference passages retrieved per coverage query. Absent → the system
    /// default.
    pub per_query_top_k: Option<u32>,
    /// Max deduped grounding passages kept across all queries. Absent → the
    /// system default.
    pub max_grounding: Option<usize>,
}

/// The `[compose]` table as it appears on disk: the long-form composition
/// pipeline's tuning knobs, mirroring `monomyth-compose::ComposeSettings`.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ComposeSection {
    /// The maximum number of model turns spent narrating a single outline
    /// section. Absent → the system default.
    pub max_turns: Option<usize>,
    /// The number of REFERENCE-path passages retrieved as grounding for each
    /// outline section. Absent → the system default.
    pub grounding_top_k: Option<u32>,
    /// The minimum cosine similarity a drafted section must reach to be
    /// accepted without revision. Absent → the system default.
    pub revise_threshold: Option<f64>,
    /// The maximum number of revision attempts made after the initial draft.
    /// Absent → the system default.
    pub max_revise_iterations: Option<usize>,
}

/// The fully-resolved configuration: every value tagged with the [`Layered`] source
/// that set it.
#[derive(Clone, Debug)]
pub struct MonomythConfig {
    /// Resolved narrative-generation knobs.
    pub generation: GenerationSettings,
    /// Resolved per-task model routing.
    pub models: ModelsSettings,
    /// Resolved law-synthesis judge-loop knobs.
    pub synthesis: SynthesisSettings,
    /// Resolved long-form composition pipeline knobs.
    pub compose: ComposeConfigSettings,
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
    /// Resolved long-form composition model routing string.
    pub compose: Layered<String>,
}

/// Resolved law-synthesis judge-loop knobs.
#[derive(Clone, Debug)]
pub struct SynthesisSettings {
    /// Resolved max judge/refine iterations.
    pub max_iterations: Layered<u32>,
    /// Resolved reference passages retrieved per coverage query.
    pub per_query_top_k: Layered<u32>,
    /// Resolved max deduped grounding passages across all queries.
    pub max_grounding: Layered<usize>,
}

/// Resolved long-form composition pipeline knobs.
///
/// Named `ComposeConfigSettings` (rather than `ComposeSettings`) because
/// `monomyth-compose::ComposeSettings` already owns that name for the pipeline's
/// own settings struct; the two are related by projection, not identity.
#[derive(Clone, Debug)]
pub struct ComposeConfigSettings {
    /// Resolved maximum number of model turns spent narrating a single
    /// outline section.
    pub max_turns: Layered<usize>,
    /// Resolved number of grounding passages retrieved per outline section.
    pub grounding_top_k: Layered<u32>,
    /// Resolved minimum cosine similarity a drafted section must reach to be
    /// accepted without revision.
    pub revise_threshold: Layered<f64>,
    /// Resolved maximum number of revision attempts after the initial draft.
    pub max_revise_iterations: Layered<usize>,
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
                compose: Layered::system_default(DEFAULT_COMPOSE_MODEL.to_owned()),
            },
            synthesis: SynthesisSettings {
                max_iterations: Layered::system_default(DEFAULT_MAX_ITERATIONS),
                per_query_top_k: Layered::system_default(DEFAULT_PER_QUERY_TOP_K),
                max_grounding: Layered::system_default(DEFAULT_MAX_GROUNDING),
            },
            compose: ComposeConfigSettings {
                max_turns: Layered::system_default(DEFAULT_COMPOSE_MAX_TURNS),
                grounding_top_k: Layered::system_default(DEFAULT_COMPOSE_GROUNDING_TOP_K),
                revise_threshold: Layered::system_default(DEFAULT_COMPOSE_REVISE_THRESHOLD),
                max_revise_iterations: Layered::system_default(
                    DEFAULT_COMPOSE_MAX_REVISE_ITERATIONS,
                ),
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
            ModelRole::Compose => self.models.compose.get(),
        }
    }

    /// Reject a resolved configuration whose values are out of range, before any of
    /// them can reach a generation pass.
    ///
    /// Checks the permille bound and each `min <= max` bound. An inverted range
    /// would otherwise panic or silently empty an RNG draw downstream, so this is
    /// the config crate's system boundary per the input-validation rule.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Invalid`] naming the first offending `section.key`.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let permille = *self.generation.fork_chance_permille.get();
        if permille > PERMILLE_MAX {
            return Err(ConfigError::Invalid {
                field: "generation.fork_chance_permille".to_owned(),
                reason: format!("must be at most {PERMILLE_MAX}, got {permille}"),
            });
        }
        Self::check_range(
            "generation.beats_per_stage",
            *self.generation.beats_per_stage_min.get(),
            *self.generation.beats_per_stage_max.get(),
        )?;
        Self::check_range(
            "generation.rooms",
            *self.generation.rooms_min.get(),
            *self.generation.rooms_max.get(),
        )?;
        Self::check_range(
            "generation.items",
            *self.generation.items_min.get(),
            *self.generation.items_max.get(),
        )?;
        let max_turns = *self.compose.max_turns.get();
        if max_turns < 1 {
            return Err(ConfigError::Invalid {
                field: "compose.max_turns".to_owned(),
                reason: format!("must be at least 1, got {max_turns}"),
            });
        }
        let grounding_top_k = *self.compose.grounding_top_k.get();
        if grounding_top_k < 1 {
            return Err(ConfigError::Invalid {
                field: "compose.grounding_top_k".to_owned(),
                reason: format!("must be at least 1, got {grounding_top_k}"),
            });
        }
        let revise_threshold = *self.compose.revise_threshold.get();
        if !revise_threshold.is_finite() {
            return Err(ConfigError::Invalid {
                field: "compose.revise_threshold".to_owned(),
                reason: format!("must be finite, got {revise_threshold}"),
            });
        }
        Ok(())
    }

    /// Reject an inverted `min`/`max` pair, naming the shared `stem` (the error
    /// reports `<stem>_min`).
    fn check_range(stem: &str, min: usize, max: usize) -> Result<(), ConfigError> {
        if min > max {
            return Err(ConfigError::Invalid {
                field: format!("{stem}_min"),
                reason: format!("must not exceed {stem}_max ({min} > {max})"),
            });
        }
        Ok(())
    }

    /// Fold one parsed configuration file into this resolved config at layer
    /// `from`. Each field's [`Layered::override_with`] enforces precedence, so the
    /// call order of layers does not matter — only their [`LayerSource`] rank.
    ///
    /// NOTE: every `Option<T>` field on a section needs a matching `override_with`
    /// line here — there is no compiler-enforced exhaustiveness, so a new file field
    /// added without a line here would silently be ignored from every file layer.
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
        self.models
            .compose
            .override_with(file.models.compose.clone(), from);
        self.synthesis
            .max_iterations
            .override_with(file.synthesis.max_iterations, from);
        self.synthesis
            .per_query_top_k
            .override_with(file.synthesis.per_query_top_k, from);
        self.synthesis
            .max_grounding
            .override_with(file.synthesis.max_grounding, from);
        self.compose
            .max_turns
            .override_with(file.compose.max_turns, from);
        self.compose
            .grounding_top_k
            .override_with(file.compose.grounding_top_k, from);
        self.compose
            .revise_threshold
            .override_with(file.compose.revise_threshold, from);
        self.compose
            .max_revise_iterations
            .override_with(file.compose.max_revise_iterations, from);
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_BEATS_PER_STAGE_MAX, DEFAULT_BEATS_PER_STAGE_MIN, DEFAULT_COMPOSE_GROUNDING_TOP_K,
        DEFAULT_COMPOSE_MAX_REVISE_ITERATIONS, DEFAULT_COMPOSE_MAX_TURNS, DEFAULT_COMPOSE_MODEL,
        DEFAULT_COMPOSE_REVISE_THRESHOLD, DEFAULT_CONTENT_MODEL, DEFAULT_FORK_CHANCE_PERMILLE,
        DEFAULT_ITEMS_MAX, DEFAULT_ITEMS_MIN, DEFAULT_MAX_EXTRA_CAST, DEFAULT_MAX_GROUNDING,
        DEFAULT_MAX_ITERATIONS, DEFAULT_PER_QUERY_TOP_K, DEFAULT_ROOMS_MAX, DEFAULT_ROOMS_MIN,
        DEFAULT_SYNTHESIS_MODEL, ModelRole, MonomythConfig, MonomythConfigFile,
    };
    use crate::error::ConfigError;
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
        assert_eq!(config.model_for(ModelRole::Compose), DEFAULT_COMPOSE_MODEL);
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
    fn default_resolves_the_system_default_synthesis_knobs() {
        let config = MonomythConfig::default();
        assert_eq!(
            *config.synthesis.max_iterations.get(),
            DEFAULT_MAX_ITERATIONS
        );
        assert_eq!(
            *config.synthesis.per_query_top_k.get(),
            DEFAULT_PER_QUERY_TOP_K
        );
        assert_eq!(*config.synthesis.max_grounding.get(), DEFAULT_MAX_GROUNDING);
    }

    #[test]
    fn a_parsed_file_overrides_synthesis_knobs_at_its_layer() {
        let file: MonomythConfigFile = toml::from_str(
            "[synthesis]\nmax_iterations = 5\nper_query_top_k = 12\nmax_grounding = 40\n",
        )
        .expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::UserOverride);
        assert_eq!(*config.synthesis.max_iterations.get(), 5);
        assert_eq!(*config.synthesis.per_query_top_k.get(), 12);
        assert_eq!(*config.synthesis.max_grounding.get(), 40);
        assert_eq!(
            config.synthesis.max_iterations.source(),
            LayerSource::UserOverride
        );
    }

    #[test]
    fn default_resolves_the_system_default_compose_knobs() {
        let config = MonomythConfig::default();
        assert_eq!(*config.compose.max_turns.get(), DEFAULT_COMPOSE_MAX_TURNS);
        assert_eq!(
            *config.compose.grounding_top_k.get(),
            DEFAULT_COMPOSE_GROUNDING_TOP_K
        );
        assert!(
            (*config.compose.revise_threshold.get() - DEFAULT_COMPOSE_REVISE_THRESHOLD).abs()
                < f64::EPSILON
        );
        assert_eq!(
            *config.compose.max_revise_iterations.get(),
            DEFAULT_COMPOSE_MAX_REVISE_ITERATIONS
        );
    }

    #[test]
    fn a_parsed_file_overrides_compose_knobs_at_its_layer() {
        let file: MonomythConfigFile = toml::from_str(
            "[compose]\nmax_turns = 8\ngrounding_top_k = 6\nrevise_threshold = 0.75\n\
             max_revise_iterations = 4\n",
        )
        .expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::UserOverride);
        assert_eq!(*config.compose.max_turns.get(), 8);
        assert_eq!(*config.compose.grounding_top_k.get(), 6);
        assert!((*config.compose.revise_threshold.get() - 0.75).abs() < f64::EPSILON);
        assert_eq!(*config.compose.max_revise_iterations.get(), 4);
        assert_eq!(config.compose.max_turns.source(), LayerSource::UserOverride);
    }

    #[test]
    fn a_parsed_file_overrides_the_compose_model_leaving_others_default() {
        let file: MonomythConfigFile =
            toml::from_str("[models]\ncompose = \"anthropic/claude-haiku-4-5\"\n")
                .expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        assert_eq!(
            config.model_for(ModelRole::Compose),
            "anthropic/claude-haiku-4-5"
        );
        assert_eq!(config.models.compose.source(), LayerSource::ProjectOverride);
        assert_eq!(config.model_for(ModelRole::Content), DEFAULT_CONTENT_MODEL);
        assert_eq!(
            config.model_for(ModelRole::Synthesis),
            DEFAULT_SYNTHESIS_MODEL
        );
    }

    #[test]
    fn an_unknown_compose_key_is_a_hard_parse_error() {
        let result: Result<MonomythConfigFile, _> = toml::from_str("[compose]\nmax_trns = 8\n");
        assert!(
            result.is_err(),
            "a misspelled [compose] key must be rejected"
        );
    }

    #[test]
    fn a_zero_compose_max_turns_is_rejected() {
        let file: MonomythConfigFile =
            toml::from_str("[compose]\nmax_turns = 0\n").expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        let error = config
            .validate()
            .expect_err("0 max_turns can never produce content");
        assert!(
            matches!(error, ConfigError::Invalid { ref field, .. } if field == "compose.max_turns"),
            "the error must name compose.max_turns, got {error:?}"
        );
    }

    #[test]
    fn a_zero_compose_grounding_top_k_is_rejected() {
        let file: MonomythConfigFile =
            toml::from_str("[compose]\ngrounding_top_k = 0\n").expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        let error = config
            .validate()
            .expect_err("0 grounding_top_k retrieves no priors to ground on");
        assert!(
            matches!(error, ConfigError::Invalid { ref field, .. } if field == "compose.grounding_top_k"),
            "the error must name compose.grounding_top_k, got {error:?}"
        );
    }

    #[test]
    fn a_non_finite_revise_threshold_is_rejected() {
        let file: MonomythConfigFile =
            toml::from_str("[compose]\nrevise_threshold = nan\n").expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        let error = config
            .validate()
            .expect_err("a NaN revise_threshold is invalid");
        assert!(
            matches!(error, ConfigError::Invalid { ref field, .. } if field == "compose.revise_threshold"),
            "the error must name compose.revise_threshold, got {error:?}"
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

    #[test]
    fn an_unknown_models_key_is_a_hard_parse_error() {
        let result: Result<MonomythConfigFile, _> =
            toml::from_str("[models]\ncontnet = \"gemini/x\"\n");
        assert!(
            result.is_err(),
            "a misspelled [models] key must be rejected"
        );
    }

    #[test]
    fn an_unknown_synthesis_key_is_a_hard_parse_error() {
        let result: Result<MonomythConfigFile, _> =
            toml::from_str("[synthesis]\nmax_iteratons = 5\n");
        assert!(
            result.is_err(),
            "a misspelled [synthesis] key must be rejected"
        );
    }

    #[test]
    fn a_stray_top_level_table_is_a_hard_parse_error() {
        let result: Result<MonomythConfigFile, _> = toml::from_str("[retrieval]\ntop_k = 5\n");
        assert!(
            result.is_err(),
            "an unknown top-level table must be rejected until its section lands"
        );
    }

    #[test]
    fn the_default_config_validates() {
        MonomythConfig::default()
            .validate()
            .expect("the system default must be in range");
    }

    #[test]
    fn a_permille_over_one_thousand_is_rejected() {
        let file: MonomythConfigFile =
            toml::from_str("[generation]\nfork_chance_permille = 1001\n").expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        let error = config
            .validate()
            .expect_err("1001 permille is out of range");
        assert!(
            matches!(error, ConfigError::Invalid { ref field, .. } if field == "generation.fork_chance_permille"),
            "the error must name the offending field, got {error:?}"
        );
    }

    #[test]
    fn an_inverted_rooms_bound_is_rejected() {
        let file: MonomythConfigFile =
            toml::from_str("[generation]\nrooms_min = 9\nrooms_max = 5\n").expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        let error = config
            .validate()
            .expect_err("rooms_min > rooms_max is invalid");
        assert!(
            matches!(error, ConfigError::Invalid { ref field, .. } if field == "generation.rooms_min"),
            "the error must name generation.rooms_min, got {error:?}"
        );
    }

    #[test]
    fn an_inverted_items_bound_is_rejected() {
        let file: MonomythConfigFile =
            toml::from_str("[generation]\nitems_min = 6\nitems_max = 2\n").expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        assert!(
            config.validate().is_err(),
            "items_min > items_max is invalid"
        );
    }

    #[test]
    fn an_inverted_beats_bound_is_rejected() {
        let file: MonomythConfigFile =
            toml::from_str("[generation]\nbeats_per_stage_min = 3\nbeats_per_stage_max = 1\n")
                .expect("valid toml");
        let mut config = MonomythConfig::default();
        config.apply_file(&file, LayerSource::ProjectOverride);
        assert!(
            config.validate().is_err(),
            "beats_per_stage_min > beats_per_stage_max is invalid"
        );
    }
}
