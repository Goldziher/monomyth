use std::path::PathBuf;

use super::{
    DEFAULT_BEATS_PER_STAGE_MAX, DEFAULT_BEATS_PER_STAGE_MIN, DEFAULT_COMPOSE_GROUNDING_TOP_K,
    DEFAULT_COMPOSE_MAX_REVISE_ITERATIONS, DEFAULT_COMPOSE_MAX_TURNS, DEFAULT_COMPOSE_MODEL,
    DEFAULT_COMPOSE_REVISE_THRESHOLD, DEFAULT_CONTENT_MODEL, DEFAULT_CORPUS_DIR, DEFAULT_DB_PATH,
    DEFAULT_DRAFT_MODEL, DEFAULT_FORK_CHANCE_PERMILLE, DEFAULT_GENRE_GROUNDING_TOP_K,
    DEFAULT_GENRE_NAME, DEFAULT_ITEMS_MAX, DEFAULT_ITEMS_MIN, DEFAULT_JUDGE_MODEL,
    DEFAULT_LAWS_DIR, DEFAULT_MAX_EXTRA_CAST, DEFAULT_MAX_GROUNDING, DEFAULT_MAX_ITERATIONS,
    DEFAULT_PER_QUERY_TOP_K, DEFAULT_REFERENCE_OVERFETCH, DEFAULT_RENDER_MEDIUM,
    DEFAULT_RETRIEVAL_TOP_K, DEFAULT_REVISE_MODEL, DEFAULT_ROOMS_MAX, DEFAULT_ROOMS_MIN,
    DEFAULT_RRF_K, DEFAULT_SYNTHESIS_MODEL, ModelRole, MonomythConfig, MonomythConfigFile,
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
        toml::from_str("[models]\ncontent = \"anthropic/claude-haiku-4-5\"\n").expect("valid toml");
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
        toml::from_str("[models]\ncompose = \"anthropic/claude-haiku-4-5\"\n").expect("valid toml");
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
    let result: Result<MonomythConfigFile, _> = toml::from_str("[synthesis]\nmax_iteratons = 5\n");
    assert!(
        result.is_err(),
        "a misspelled [synthesis] key must be rejected"
    );
}

#[test]
fn default_resolves_the_system_default_genre_knobs() {
    let config = MonomythConfig::default();
    assert_eq!(config.genre.name.get(), DEFAULT_GENRE_NAME);
    assert_eq!(
        *config.genre.grounding_top_k.get(),
        DEFAULT_GENRE_GROUNDING_TOP_K
    );
    assert_eq!(config.genre.name.source(), LayerSource::SystemDefault);
}

#[test]
fn a_parsed_file_overrides_genre_knobs_at_its_layer() {
    let file: MonomythConfigFile =
        toml::from_str("[genre]\nname = \"detective\"\ngrounding_top_k = 6\n").expect("valid toml");
    let mut config = MonomythConfig::default();
    config.apply_file(&file, LayerSource::ProjectOverride);
    assert_eq!(config.genre.name.get(), "detective");
    assert_eq!(*config.genre.grounding_top_k.get(), 6);
    assert_eq!(config.genre.name.source(), LayerSource::ProjectOverride);
}

#[test]
fn unknown_genre_key_is_a_hard_parse_error() {
    let result: Result<MonomythConfigFile, _> = toml::from_str("[genre]\nnaem = \"detective\"\n");
    assert!(result.is_err(), "a misspelled [genre] key must be rejected");
}

#[test]
fn a_zero_genre_grounding_top_k_is_rejected() {
    let file: MonomythConfigFile =
        toml::from_str("[genre]\ngrounding_top_k = 0\n").expect("valid toml");
    let mut config = MonomythConfig::default();
    config.apply_file(&file, LayerSource::ProjectOverride);
    let error = config
        .validate()
        .expect_err("0 grounding_top_k retrieves no priors to ground on");
    assert!(
        matches!(error, ConfigError::Invalid { ref field, .. } if field == "genre.grounding_top_k"),
        "the error must name genre.grounding_top_k, got {error:?}"
    );
}

#[test]
fn default_resolves_the_system_default_render_medium() {
    let config = MonomythConfig::default();
    assert_eq!(config.render.medium.get(), DEFAULT_RENDER_MEDIUM);
    assert_eq!(config.render.medium.source(), LayerSource::SystemDefault);
}

#[test]
fn a_parsed_file_overrides_render_medium_at_its_layer() {
    let file: MonomythConfigFile =
        toml::from_str("[render]\nmedium = \"terse\"\n").expect("valid toml");
    let mut config = MonomythConfig::default();
    config.apply_file(&file, LayerSource::ProjectOverride);
    assert_eq!(config.render.medium.get(), "terse");
    assert_eq!(config.render.medium.source(), LayerSource::ProjectOverride);
}

#[test]
fn unknown_render_key_is_a_hard_parse_error() {
    let result: Result<MonomythConfigFile, _> = toml::from_str("[render]\nmedum = \"terse\"\n");
    assert!(
        result.is_err(),
        "a misspelled [render] key must be rejected"
    );
}

#[test]
fn a_stray_top_level_table_is_a_hard_parse_error() {
    let result: Result<MonomythConfigFile, _> = toml::from_str("[nonexistent]\nkey = 5\n");
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

#[test]
fn default_resolves_the_system_default_retrieval_knobs() {
    let config = MonomythConfig::default();
    assert_eq!(*config.retrieval.top_k.get(), DEFAULT_RETRIEVAL_TOP_K);
    assert_eq!(
        *config.retrieval.reference_overfetch.get(),
        DEFAULT_REFERENCE_OVERFETCH
    );
    assert!((*config.retrieval.rrf_k.get() - DEFAULT_RRF_K).abs() < f64::EPSILON);
    assert_eq!(config.retrieval.top_k.source(), LayerSource::SystemDefault);
}

#[test]
fn a_parsed_file_overrides_retrieval_knobs_at_its_layer() {
    let file: MonomythConfigFile =
        toml::from_str("[retrieval]\ntop_k = 10\nreference_overfetch = 5\nrrf_k = 30.0\n")
            .expect("valid toml");
    let mut config = MonomythConfig::default();
    config.apply_file(&file, LayerSource::ProjectOverride);
    assert_eq!(*config.retrieval.top_k.get(), 10);
    assert_eq!(*config.retrieval.reference_overfetch.get(), 5);
    assert!((*config.retrieval.rrf_k.get() - 30.0).abs() < f64::EPSILON);
    assert_eq!(
        config.retrieval.top_k.source(),
        LayerSource::ProjectOverride
    );
}

#[test]
fn unknown_retrieval_key_is_a_hard_parse_error() {
    let result: Result<MonomythConfigFile, _> = toml::from_str("[retrieval]\ntpo_k = 5\n");
    assert!(
        result.is_err(),
        "a misspelled [retrieval] key must be rejected"
    );
}

#[test]
fn a_zero_retrieval_top_k_is_rejected() {
    let file: MonomythConfigFile = toml::from_str("[retrieval]\ntop_k = 0\n").expect("valid toml");
    let mut config = MonomythConfig::default();
    config.apply_file(&file, LayerSource::ProjectOverride);
    let error = config.validate().expect_err("0 top_k retrieves nothing");
    assert!(
        matches!(error, ConfigError::Invalid { ref field, .. } if field == "retrieval.top_k"),
        "the error must name retrieval.top_k, got {error:?}"
    );
}

#[test]
fn a_zero_reference_overfetch_is_rejected() {
    let file: MonomythConfigFile =
        toml::from_str("[retrieval]\nreference_overfetch = 0\n").expect("valid toml");
    let mut config = MonomythConfig::default();
    config.apply_file(&file, LayerSource::ProjectOverride);
    let error = config
        .validate()
        .expect_err("0 reference_overfetch would fetch nothing to dedupe");
    assert!(
        matches!(error, ConfigError::Invalid { ref field, .. } if field == "retrieval.reference_overfetch"),
        "the error must name retrieval.reference_overfetch, got {error:?}"
    );
}

#[test]
fn a_non_positive_rrf_k_is_rejected() {
    let file: MonomythConfigFile =
        toml::from_str("[retrieval]\nrrf_k = 0.0\n").expect("valid toml");
    let mut config = MonomythConfig::default();
    config.apply_file(&file, LayerSource::ProjectOverride);
    let error = config
        .validate()
        .expect_err("a zero rrf_k divides ranking by zero at rank 0");
    assert!(
        matches!(error, ConfigError::Invalid { ref field, .. } if field == "retrieval.rrf_k"),
        "the error must name retrieval.rrf_k, got {error:?}"
    );
}

#[test]
fn default_resolves_the_system_default_paths() {
    let config = MonomythConfig::default();
    assert_eq!(
        config.paths.corpus_dir.get(),
        &PathBuf::from(DEFAULT_CORPUS_DIR)
    );
    assert_eq!(
        config.paths.laws_dir.get(),
        &PathBuf::from(DEFAULT_LAWS_DIR)
    );
    assert_eq!(config.paths.db_path.get(), &PathBuf::from(DEFAULT_DB_PATH));
    assert_eq!(config.paths.corpus_dir.source(), LayerSource::SystemDefault);
}

#[test]
fn a_parsed_file_overrides_paths_at_its_layer() {
    let file: MonomythConfigFile = toml::from_str(
            "[paths]\ncorpus_dir = \"data/raw\"\nlaws_dir = \"data/laws\"\ndb_path = \"data/store.db\"\n",
        )
        .expect("valid toml");
    let mut config = MonomythConfig::default();
    config.apply_file(&file, LayerSource::ProjectOverride);
    assert_eq!(config.paths.corpus_dir.get(), &PathBuf::from("data/raw"));
    assert_eq!(config.paths.laws_dir.get(), &PathBuf::from("data/laws"));
    assert_eq!(config.paths.db_path.get(), &PathBuf::from("data/store.db"));
    assert_eq!(
        config.paths.corpus_dir.source(),
        LayerSource::ProjectOverride
    );
}

#[test]
fn unknown_paths_key_is_a_hard_parse_error() {
    let result: Result<MonomythConfigFile, _> =
        toml::from_str("[paths]\ncorpsu_dir = \"data/raw\"\n");
    assert!(result.is_err(), "a misspelled [paths] key must be rejected");
}

#[test]
fn default_resolves_the_system_default_judge_loop_models() {
    let config = MonomythConfig::default();
    assert_eq!(config.model_for(ModelRole::Draft), DEFAULT_DRAFT_MODEL);
    assert_eq!(config.model_for(ModelRole::Judge), DEFAULT_JUDGE_MODEL);
    assert_eq!(config.model_for(ModelRole::Revise), DEFAULT_REVISE_MODEL);
}

#[test]
fn a_parsed_file_overrides_the_judge_model_leaving_the_others_default() {
    let file: MonomythConfigFile =
        toml::from_str("[models]\njudge = \"anthropic/claude-opus-4-8\"\n").expect("valid toml");
    let mut config = MonomythConfig::default();
    config.apply_file(&file, LayerSource::ProjectOverride);
    assert_eq!(
        config.model_for(ModelRole::Judge),
        "anthropic/claude-opus-4-8"
    );
    assert_eq!(config.models.judge.source(), LayerSource::ProjectOverride);
    assert_eq!(
        config.model_for(ModelRole::Draft),
        DEFAULT_DRAFT_MODEL,
        "an untouched task falls back to its own default"
    );
    assert_eq!(
        config.model_for(ModelRole::Revise),
        DEFAULT_REVISE_MODEL,
        "an untouched task falls back to its own default"
    );
    assert_eq!(
        config.model_for(ModelRole::Synthesis),
        DEFAULT_SYNTHESIS_MODEL,
        "an untouched task falls back to its own default"
    );
}
