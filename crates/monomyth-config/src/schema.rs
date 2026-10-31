//! The configuration schema: the on-disk file shape ([`MonomythConfigFile`]) and
//! the resolved, layer-tagged shape ([`MonomythConfig`]).
//!
//! This is the config spine (ADR-0015). Each knob appears three times: an
//! `Option`-typed field on the file struct, a `Layered<T>` field on the resolved
//! struct, and one line in [`MonomythConfig::apply_file`]. It carries the
//! `[generation]` table (the macro fork probability plus the beat, map, item,
//! and cast procedural bounds), the `[models]` table (per-task model routing),
//! the `[synthesis]`/`[compose]`/`[genre]`/`[render]` tables, and the
//! `[retrieval]`/`[paths]` tables; a later slice adds `[knowledge]` the same
//! way.

use std::path::PathBuf;

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

/// The system-default `provider/model` routing string for the drafting phase of
/// the law-synthesis judge loop (ADR-0024's initial-candidate distillation
/// call).
///
/// Equals [`DEFAULT_SYNTHESIS_MODEL`]: `monomyth-synthesis::draft_law` today
/// drives drafting, judging, and refining from a single `Llm` instance, so all
/// three roles start pinned to the same model. Each is independently
/// overridable so a future widening of `draft_law`'s API to accept a model per
/// phase can route them apart without another config migration.
const DEFAULT_DRAFT_MODEL: &str = DEFAULT_SYNTHESIS_MODEL;

/// The system-default `provider/model` routing string for the judging phase of
/// the law-synthesis judge loop. See [`DEFAULT_DRAFT_MODEL`] for why it starts
/// equal to [`DEFAULT_SYNTHESIS_MODEL`].
const DEFAULT_JUDGE_MODEL: &str = DEFAULT_SYNTHESIS_MODEL;

/// The system-default `provider/model` routing string for the refine phase of
/// the law-synthesis judge loop. See [`DEFAULT_DRAFT_MODEL`] for why it starts
/// equal to [`DEFAULT_SYNTHESIS_MODEL`].
const DEFAULT_REVISE_MODEL: &str = DEFAULT_SYNTHESIS_MODEL;

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

/// The system default for `genre.name`: the myth flavor, i.e. the behavior every
/// content-fill pass had before genre targeting existed.
///
/// Duplicates `monomyth-genre`'s `GenreProfile` default for the duration of the
/// migration; the two must agree, guarded by `monomyth-genre`'s test asserting the
/// resolved-default genre config equals `GenreProfile::default()`.
/// `monomyth-config` does not depend on `monomyth-genre`, so the value cannot
/// simply be imported.
const DEFAULT_GENRE_NAME: &str = "myth";
/// The system default for `genre.grounding_top_k`.
///
/// Duplicates `monomyth-gen`'s `ContentConfig::default` grounding breadth so an
/// unconfigured myth profile grounds identically to a run with no genre targeting
/// at all.
const DEFAULT_GENRE_GROUNDING_TOP_K: u32 = 4;

/// The system default for `render.medium`: the prose frontend, i.e. the only
/// renderer that existed before medium selection was configurable (ADR-0020).
///
/// `monomyth-config` does not depend on `monomyth-contracts` or any renderer
/// crate, so the medium is a plain string here; the composition root
/// (`monomyth-cli`) is the one place that maps it to a concrete
/// `Box<dyn Renderer>`, mirroring how [`DEFAULT_GENRE_NAME`] is projected into a
/// `monomyth-genre::GenreKind` outside this crate.
const DEFAULT_RENDER_MEDIUM: &str = "prose";

/// The system default for `retrieval.top_k`.
///
/// Duplicates `monomyth-cli`'s `DEFAULT_TOP_K` (the `retrieve` command's default
/// passage count); the two must agree for the duration of the migration.
/// `monomyth-config` does not depend on `monomyth-cli`, so the value cannot
/// simply be imported.
const DEFAULT_RETRIEVAL_TOP_K: u32 = 5;

/// The system default for `retrieval.reference_overfetch`.
///
/// Duplicates `monomyth-knowledge`'s `REFERENCE_OVERFETCH`: the reference
/// retrieval path requests `top_k * reference_overfetch` candidates so
/// near-duplicate chunks from one long, overlap-chunked source can be dropped
/// before the final `top_k` is taken. `monomyth-config` does not depend on
/// `monomyth-knowledge`, so the value cannot simply be imported.
const DEFAULT_REFERENCE_OVERFETCH: u32 = 3;

/// The system default for `retrieval.rrf_k`.
///
/// Duplicates `monomyth-knowledge`'s `K_RRF` (the canonical reciprocal-rank-fusion
/// constant, `k = 60`, widened from `f32` to `f64` for the resolved config's
/// floating-point convention). `monomyth-config` does not depend on
/// `monomyth-knowledge`, so the value cannot simply be imported.
const DEFAULT_RRF_K: f64 = 60.0;

/// The system default for `paths.corpus_dir`.
///
/// Duplicates `monomyth-knowledge::acquire::storage`'s `CACHE_DIR` (the on-disk
/// raw-fetch cache directory, relative to the current working directory).
/// `monomyth-config` does not depend on `monomyth-knowledge`, so the value
/// cannot simply be imported.
const DEFAULT_CORPUS_DIR: &str = "corpus/raw";

/// The system default for `paths.laws_dir`.
///
/// Duplicates `monomyth-cli`'s `DEFAULT_LAWS_DIR` (where `synthesize law`
/// promotes a reviewed `LawArtifact`). `monomyth-config` does not depend on
/// `monomyth-cli`, so the value cannot simply be imported.
const DEFAULT_LAWS_DIR: &str = "./artifacts/laws";

/// The system default for `paths.db_path`.
///
/// Duplicates `monomyth-cli`'s `DEFAULT_DB` (the global `--db` flag's default).
/// `monomyth-config` does not depend on `monomyth-cli`, so the value cannot
/// simply be imported.
const DEFAULT_DB_PATH: &str = "./monomyth.db";

/// A per-task model role: which configured model a caller resolves via
/// [`MonomythConfig::model_for`].
///
/// Splitting model selection by role is what lets a cheap tier drive content fill
/// while a quality tier drives synthesis, from one config table. New roles (an
/// extractor) are added here as their call sites migrate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModelRole {
    /// The per-slot content-fill pass (`gen --fill`).
    Content,
    /// The law-synthesis distillation pass (`synthesize law`), overall.
    Synthesis,
    /// The long-form composition pipeline (`compose`).
    Compose,
    /// The drafting phase of the law-synthesis judge loop (ADR-0024).
    Draft,
    /// The judging phase of the law-synthesis judge loop (ADR-0024).
    Judge,
    /// The refine phase of the law-synthesis judge loop (ADR-0024).
    Revise,
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
    /// The `[genre]` table.
    pub genre: GenreSection,
    /// The `[render]` table.
    pub render: RenderSection,
    /// The `[retrieval]` table.
    pub retrieval: RetrievalSection,
    /// The `[paths]` table.
    pub paths: PathsSection,
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
    /// Overrides the law-synthesis judge loop's drafting-phase model when
    /// present.
    pub draft: Option<String>,
    /// Overrides the law-synthesis judge loop's judging-phase model when
    /// present.
    pub judge: Option<String>,
    /// Overrides the law-synthesis judge loop's refine-phase model when
    /// present.
    pub revise: Option<String>,
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

/// The `[genre]` table as it appears on disk: which genre profile biases
/// content-fill targeting, mirroring `monomyth-genre::GenreProfile`.
///
/// Kept as plain scalars (a name string, a numeric knob) rather than an enum or
/// the `monomyth-genre` type itself, so `monomyth-config` never depends on
/// `monomyth-genre` — the dependency runs the other way, `GenreProfile` is
/// constructed *from* the resolved [`GenreSettings`].
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct GenreSection {
    /// The genre identifier (e.g. `"myth"`, `"detective"`, `"litrpg"`) that
    /// selects a `monomyth-genre::GenreProfile`. Absent → the system default.
    pub name: Option<String>,
    /// Number of ship-safe passages retrieved to ground each content slot under
    /// this genre. Absent → the system default.
    pub grounding_top_k: Option<u32>,
}

/// The `[render]` table as it appears on disk: which `Renderer` medium
/// (`monomyth-contracts`'s P-RENDER seam, ADR-0020) the composition root
/// selects, mirroring `[genre]`'s shape.
///
/// Kept as a plain string rather than an enum or a `monomyth-contracts`/renderer
/// crate type, so `monomyth-config` never gains an edge onto `monomyth-contracts`
/// or any renderer crate — the dependency runs the other way: the composition
/// root reads this resolved string and constructs the concrete `Box<dyn
/// Renderer>` from it.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RenderSection {
    /// The renderer medium identifier (e.g. `"prose"`, `"terse"`) that selects a
    /// `Box<dyn Renderer>` at the composition root. Absent → the system default.
    pub medium: Option<String>,
}

/// The `[retrieval]` table as it appears on disk: ad-hoc/hybrid retrieval
/// tuning shared by the `retrieve` command and `monomyth-knowledge`'s hybrid
/// (vector + full-text) search path.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RetrievalSection {
    /// Maximum number of passages an ad-hoc retrieval returns. Absent → the
    /// system default.
    pub top_k: Option<u32>,
    /// Over-fetch factor for reference-path retrieval: candidates requested are
    /// `top_k * reference_overfetch`, so near-duplicate chunks can be dropped
    /// before the final `top_k` is taken. Absent → the system default.
    pub reference_overfetch: Option<u32>,
    /// The reciprocal-rank-fusion constant blending vector, full-text, and
    /// sparse hit rankings in hybrid search. Absent → the system default.
    pub rrf_k: Option<f64>,
}

/// The `[paths]` table as it appears on disk: filesystem locations the CLI and
/// knowledge layer read from and write to.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct PathsSection {
    /// The on-disk raw-fetch cache directory, relative to the current working
    /// directory unless absolute. Absent → the system default.
    pub corpus_dir: Option<PathBuf>,
    /// The directory reviewed `LawArtifact`s are promoted into. Absent → the
    /// system default.
    pub laws_dir: Option<PathBuf>,
    /// The knowledge-layer database file. Absent → the system default.
    pub db_path: Option<PathBuf>,
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
    /// Resolved genre-targeting knobs.
    pub genre: GenreSettings,
    /// Resolved renderer-medium selection.
    pub render: RenderSettings,
    /// Resolved retrieval tuning.
    pub retrieval: RetrievalSettings,
    /// Resolved filesystem paths.
    pub paths: PathsSettings,
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
    /// Resolved law-synthesis judge loop drafting-phase model routing string.
    pub draft: Layered<String>,
    /// Resolved law-synthesis judge loop judging-phase model routing string.
    pub judge: Layered<String>,
    /// Resolved law-synthesis judge loop refine-phase model routing string.
    pub revise: Layered<String>,
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

/// Resolved genre-targeting knobs.
#[derive(Clone, Debug)]
pub struct GenreSettings {
    /// Resolved genre identifier (e.g. `"myth"`, `"detective"`, `"litrpg"`).
    pub name: Layered<String>,
    /// Resolved number of grounding passages retrieved per content slot under
    /// this genre.
    pub grounding_top_k: Layered<u32>,
}

/// Resolved renderer-medium selection.
#[derive(Clone, Debug)]
pub struct RenderSettings {
    /// Resolved renderer medium identifier (e.g. `"prose"`, `"terse"`).
    pub medium: Layered<String>,
}

/// Resolved retrieval tuning.
#[derive(Clone, Debug)]
pub struct RetrievalSettings {
    /// Resolved maximum number of passages an ad-hoc retrieval returns.
    pub top_k: Layered<u32>,
    /// Resolved reference-path over-fetch factor.
    pub reference_overfetch: Layered<u32>,
    /// Resolved reciprocal-rank-fusion constant.
    pub rrf_k: Layered<f64>,
}

/// Resolved filesystem paths.
#[derive(Clone, Debug)]
pub struct PathsSettings {
    /// Resolved on-disk raw-fetch cache directory.
    pub corpus_dir: Layered<PathBuf>,
    /// Resolved directory reviewed `LawArtifact`s are promoted into.
    pub laws_dir: Layered<PathBuf>,
    /// Resolved knowledge-layer database file.
    pub db_path: Layered<PathBuf>,
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
                draft: Layered::system_default(DEFAULT_DRAFT_MODEL.to_owned()),
                judge: Layered::system_default(DEFAULT_JUDGE_MODEL.to_owned()),
                revise: Layered::system_default(DEFAULT_REVISE_MODEL.to_owned()),
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
            genre: GenreSettings {
                name: Layered::system_default(DEFAULT_GENRE_NAME.to_owned()),
                grounding_top_k: Layered::system_default(DEFAULT_GENRE_GROUNDING_TOP_K),
            },
            render: RenderSettings {
                medium: Layered::system_default(DEFAULT_RENDER_MEDIUM.to_owned()),
            },
            retrieval: RetrievalSettings {
                top_k: Layered::system_default(DEFAULT_RETRIEVAL_TOP_K),
                reference_overfetch: Layered::system_default(DEFAULT_REFERENCE_OVERFETCH),
                rrf_k: Layered::system_default(DEFAULT_RRF_K),
            },
            paths: PathsSettings {
                corpus_dir: Layered::system_default(PathBuf::from(DEFAULT_CORPUS_DIR)),
                laws_dir: Layered::system_default(PathBuf::from(DEFAULT_LAWS_DIR)),
                db_path: Layered::system_default(PathBuf::from(DEFAULT_DB_PATH)),
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
            ModelRole::Draft => self.models.draft.get(),
            ModelRole::Judge => self.models.judge.get(),
            ModelRole::Revise => self.models.revise.get(),
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
        let genre_grounding_top_k = *self.genre.grounding_top_k.get();
        if genre_grounding_top_k < 1 {
            return Err(ConfigError::Invalid {
                field: "genre.grounding_top_k".to_owned(),
                reason: format!("must be at least 1, got {genre_grounding_top_k}"),
            });
        }
        let retrieval_top_k = *self.retrieval.top_k.get();
        if retrieval_top_k < 1 {
            return Err(ConfigError::Invalid {
                field: "retrieval.top_k".to_owned(),
                reason: format!("must be at least 1, got {retrieval_top_k}"),
            });
        }
        let reference_overfetch = *self.retrieval.reference_overfetch.get();
        if reference_overfetch < 1 {
            return Err(ConfigError::Invalid {
                field: "retrieval.reference_overfetch".to_owned(),
                reason: format!("must be at least 1, got {reference_overfetch}"),
            });
        }
        let rrf_k = *self.retrieval.rrf_k.get();
        if !rrf_k.is_finite() || rrf_k <= 0.0 {
            return Err(ConfigError::Invalid {
                field: "retrieval.rrf_k".to_owned(),
                reason: format!("must be finite and positive, got {rrf_k}"),
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
    /// line in that section's `apply_file` — there is no compiler-enforced
    /// exhaustiveness, so a new file field added without one would silently be
    /// ignored from every file layer.
    pub(crate) fn apply_file(&mut self, file: &MonomythConfigFile, from: LayerSource) {
        self.generation.apply_file(&file.generation, from);
        self.models.apply_file(&file.models, from);
        self.synthesis.apply_file(&file.synthesis, from);
        self.compose.apply_file(&file.compose, from);
        self.genre.apply_file(&file.genre, from);
        self.render.apply_file(&file.render, from);
        self.retrieval.apply_file(&file.retrieval, from);
        self.paths.apply_file(&file.paths, from);
    }
}

impl GenerationSettings {
    /// Layer the `[generation]` section of one config file over these settings.
    fn apply_file(&mut self, file: &GenerationSection, from: LayerSource) {
        self.fork_chance_permille
            .override_with(file.fork_chance_permille, from);
        self.beats_per_stage_min
            .override_with(file.beats_per_stage_min, from);
        self.beats_per_stage_max
            .override_with(file.beats_per_stage_max, from);
        self.rooms_min.override_with(file.rooms_min, from);
        self.rooms_max.override_with(file.rooms_max, from);
        self.items_min.override_with(file.items_min, from);
        self.items_max.override_with(file.items_max, from);
        self.max_extra_cast.override_with(file.max_extra_cast, from);
    }
}

impl ModelsSettings {
    /// Layer the `[models]` section of one config file over these settings.
    fn apply_file(&mut self, file: &ModelsSection, from: LayerSource) {
        self.content.override_with(file.content.clone(), from);
        self.synthesis.override_with(file.synthesis.clone(), from);
        self.compose.override_with(file.compose.clone(), from);
        self.draft.override_with(file.draft.clone(), from);
        self.judge.override_with(file.judge.clone(), from);
        self.revise.override_with(file.revise.clone(), from);
    }
}

impl SynthesisSettings {
    /// Layer the `[synthesis]` section of one config file over these settings.
    fn apply_file(&mut self, file: &SynthesisSection, from: LayerSource) {
        self.max_iterations.override_with(file.max_iterations, from);
        self.per_query_top_k
            .override_with(file.per_query_top_k, from);
        self.max_grounding.override_with(file.max_grounding, from);
    }
}

impl ComposeConfigSettings {
    /// Layer the `[compose]` section of one config file over these settings.
    fn apply_file(&mut self, file: &ComposeSection, from: LayerSource) {
        self.max_turns.override_with(file.max_turns, from);
        self.grounding_top_k
            .override_with(file.grounding_top_k, from);
        self.revise_threshold
            .override_with(file.revise_threshold, from);
        self.max_revise_iterations
            .override_with(file.max_revise_iterations, from);
    }
}

impl GenreSettings {
    /// Layer the `[genre]` section of one config file over these settings.
    fn apply_file(&mut self, file: &GenreSection, from: LayerSource) {
        self.name.override_with(file.name.clone(), from);
        self.grounding_top_k
            .override_with(file.grounding_top_k, from);
    }
}

impl RenderSettings {
    /// Layer the `[render]` section of one config file over these settings.
    fn apply_file(&mut self, file: &RenderSection, from: LayerSource) {
        self.medium.override_with(file.medium.clone(), from);
    }
}

impl RetrievalSettings {
    /// Layer the `[retrieval]` section of one config file over these settings.
    fn apply_file(&mut self, file: &RetrievalSection, from: LayerSource) {
        self.top_k.override_with(file.top_k, from);
        self.reference_overfetch
            .override_with(file.reference_overfetch, from);
        self.rrf_k.override_with(file.rrf_k, from);
    }
}

impl PathsSettings {
    /// Layer the `[paths]` section of one config file over these settings.
    fn apply_file(&mut self, file: &PathsSection, from: LayerSource) {
        self.corpus_dir.override_with(file.corpus_dir.clone(), from);
        self.laws_dir.override_with(file.laws_dir.clone(), from);
        self.db_path.override_with(file.db_path.clone(), from);
    }
}

#[cfg(test)]
mod tests;
