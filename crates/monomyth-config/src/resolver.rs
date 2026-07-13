//! [`ConfigResolver`]: build a [`MonomythConfig`] by folding the layered sources.
//!
//! Resolution starts from the compiled [`MonomythConfig::default`]
//! ([`LayerSource::SystemDefault`]) and folds in, in precedence order, an optional
//! deployment file, the per-user file, the project file, and finally any runtime
//! (CLI) overrides. Missing files are skipped silently; a present-but-malformed
//! file is a hard [`ConfigError`], and a resolved value out of range is rejected by
//! [`ConfigResolver::resolve`].

use std::path::{Path, PathBuf};

use crate::error::ConfigError;
use crate::layered::LayerSource;
use crate::schema::{MonomythConfig, MonomythConfigFile};

/// The project-level config file, resolved relative to the current directory.
const PROJECT_CONFIG_FILE: &str = "monomyth.toml";
/// The per-user config file, resolved under the OS config directory.
const USER_CONFIG_SUBPATH: &str = "monomyth/monomyth.toml";
/// Environment variable naming a directory that holds a deployment-default
/// `monomyth.toml`.
const DEPLOYMENT_CONFIG_DIR_ENV: &str = "MONOMYTH_CONFIG_DIR";

/// Runtime overrides, i.e. values supplied on the command line. Each present field
/// becomes a [`LayerSource::RuntimeOverride`] and beats every file layer.
///
/// This is deliberately partial: it carries only the knobs that have a CLI flag
/// today (four of the schema's tunables). The file-only knobs are set through a
/// `monomyth.toml` layer, not here.
#[derive(Clone, Debug, Default)]
pub struct RuntimeOverrides {
    /// Overrides `generation.fork_chance_permille` when present.
    pub fork_chance_permille: Option<u16>,
    /// Overrides `models.content` when present (the global `--model` flag).
    pub content_model: Option<String>,
    /// Overrides `models.synthesis` when present (the `synthesize law --model` flag).
    pub synthesis_model: Option<String>,
    /// Overrides `synthesis.per_query_top_k` when present (the `synthesize law
    /// --top-k` flag).
    pub synthesis_per_query_top_k: Option<u32>,
    /// Overrides `models.compose` when present (the `compose --model` flag,
    /// which shares the global `--model` flag).
    pub compose_model: Option<String>,
}

/// Accumulates the layered configuration and yields the resolved [`MonomythConfig`].
#[derive(Clone, Debug)]
pub struct ConfigResolver {
    config: MonomythConfig,
}

impl ConfigResolver {
    /// A resolver seeded with the compiled defaults only — no filesystem access.
    ///
    /// This is the deterministic entry point for tests and for callers that must
    /// not read the disk: [`resolve`](Self::resolve) returns exactly the
    /// [`MonomythConfig::default`] table (which always validates).
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            config: MonomythConfig::default(),
        }
    }

    /// Seed with defaults, then fold in the deployment, user, and project files
    /// that exist, resolved from the process environment: `$MONOMYTH_CONFIG_DIR`,
    /// the OS config dir, and `./monomyth.toml`.
    ///
    /// A missing file is skipped; call order does not affect the result because
    /// precedence is fixed by [`LayerSource`] rank. This reads process-global state
    /// (the env var and the current directory); [`discover_from`](Self::discover_from)
    /// is the path-injected core used by tests.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Read`] if a discovered file exists but cannot be
    /// read, or [`ConfigError::Parse`] if it is not valid TOML for the schema.
    pub fn discover() -> Result<Self, ConfigError> {
        let deployment = std::env::var_os(DEPLOYMENT_CONFIG_DIR_ENV)
            .map(|dir| Path::new(&dir).join(PROJECT_CONFIG_FILE));
        let user = dirs::config_dir().map(|base| base.join(USER_CONFIG_SUBPATH));
        let project = PathBuf::from(PROJECT_CONFIG_FILE);
        Self::discover_from(deployment.as_deref(), user.as_deref(), Some(&project))
    }

    /// The path-injected core of [`discover`](Self::discover): fold in whichever of
    /// the deployment, user, and project files are `Some` and present, each at its
    /// fixed layer. Takes no global state, so it is safe under parallel tests.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Read`]/[`ConfigError::Parse`] for a present file that
    /// cannot be read or does not parse.
    pub fn discover_from(
        deployment: Option<&Path>,
        user: Option<&Path>,
        project: Option<&Path>,
    ) -> Result<Self, ConfigError> {
        let mut resolver = Self::defaults();
        if let Some(path) = deployment {
            resolver.merge_file(path, LayerSource::DeploymentDefault)?;
        }
        if let Some(path) = user {
            resolver.merge_file(path, LayerSource::UserOverride)?;
        }
        if let Some(path) = project {
            resolver.merge_file(path, LayerSource::ProjectOverride)?;
        }
        Ok(resolver)
    }

    /// Apply CLI/programmatic overrides as the top [`LayerSource::RuntimeOverride`]
    /// layer, so a flag always wins over a file.
    #[must_use]
    pub fn with_runtime(mut self, overrides: RuntimeOverrides) -> Self {
        self.config
            .generation
            .fork_chance_permille
            .override_with(overrides.fork_chance_permille, LayerSource::RuntimeOverride);
        self.config
            .models
            .content
            .override_with(overrides.content_model, LayerSource::RuntimeOverride);
        self.config
            .models
            .synthesis
            .override_with(overrides.synthesis_model, LayerSource::RuntimeOverride);
        self.config
            .models
            .compose
            .override_with(overrides.compose_model, LayerSource::RuntimeOverride);
        self.config.synthesis.per_query_top_k.override_with(
            overrides.synthesis_per_query_top_k,
            LayerSource::RuntimeOverride,
        );
        self
    }

    /// The fully-resolved configuration, validated.
    ///
    /// Validation runs here, after every layer (including runtime overrides) has
    /// been folded in, so an out-of-range value can never leave the resolver.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Invalid`] if a resolved value is out of range (a
    /// permille over `1000`, or an inverted `min`/`max` bound).
    pub fn resolve(self) -> Result<MonomythConfig, ConfigError> {
        self.config.validate()?;
        Ok(self.config)
    }

    /// Read and fold a single config file at `path` into the running config at
    /// layer `from`. A missing file is a no-op.
    fn merge_file(&mut self, path: &Path, from: LayerSource) -> Result<(), ConfigError> {
        let contents = match std::fs::read_to_string(path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => {
                return Err(ConfigError::Read {
                    path: path.to_path_buf(),
                    source: error,
                });
            }
        };
        let file: MonomythConfigFile =
            toml::from_str(&contents).map_err(|source| ConfigError::Parse {
                path: path.to_path_buf(),
                source,
            })?;
        self.config.apply_file(&file, from);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::{ConfigResolver, RuntimeOverrides};
    use crate::error::ConfigError;
    use crate::layered::LayerSource;

    /// Write `contents` to a uniquely-named file under the OS temp dir, returning
    /// its path. The `tag` keeps concurrent tests from colliding without needing a
    /// randomness source (which the scripts ban).
    fn write_temp(tag: &str, contents: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("monomyth-config-test-{tag}.toml"));
        let mut file = std::fs::File::create(&path).expect("create temp config");
        file.write_all(contents.as_bytes())
            .expect("write temp config");
        path
    }

    #[test]
    fn defaults_resolve_the_system_default() {
        let config = ConfigResolver::defaults()
            .resolve()
            .expect("defaults validate");
        assert_eq!(*config.generation.fork_chance_permille.get(), 500);
        assert_eq!(
            config.generation.fork_chance_permille.source(),
            LayerSource::SystemDefault
        );
    }

    #[test]
    fn a_runtime_override_wins_over_the_default() {
        let config = ConfigResolver::defaults()
            .with_runtime(RuntimeOverrides {
                fork_chance_permille: Some(750),
                ..RuntimeOverrides::default()
            })
            .resolve()
            .expect("in-range override validates");
        assert_eq!(*config.generation.fork_chance_permille.get(), 750);
        assert_eq!(
            config.generation.fork_chance_permille.source(),
            LayerSource::RuntimeOverride
        );
    }

    #[test]
    fn an_absent_runtime_override_leaves_the_default() {
        let config = ConfigResolver::defaults()
            .with_runtime(RuntimeOverrides::default())
            .resolve()
            .expect("defaults validate");
        assert_eq!(*config.generation.fork_chance_permille.get(), 500);
        assert_eq!(
            config.generation.fork_chance_permille.source(),
            LayerSource::SystemDefault
        );
    }

    #[test]
    fn a_runtime_model_override_wins_over_the_default() {
        use crate::schema::ModelRole;

        let config = ConfigResolver::defaults()
            .with_runtime(RuntimeOverrides {
                content_model: Some("openai/gpt-5".to_owned()),
                ..RuntimeOverrides::default()
            })
            .resolve()
            .expect("defaults validate");
        assert_eq!(config.model_for(ModelRole::Content), "openai/gpt-5");
        assert_eq!(
            config.models.content.source(),
            LayerSource::RuntimeOverride,
            "the flag advances the content model to the runtime layer",
        );
        assert_eq!(config.models.synthesis.source(), LayerSource::SystemDefault);
    }

    #[test]
    fn an_out_of_range_runtime_override_is_rejected_by_resolve() {
        let error = ConfigResolver::defaults()
            .with_runtime(RuntimeOverrides {
                fork_chance_permille: Some(2000),
                ..RuntimeOverrides::default()
            })
            .resolve()
            .expect_err("2000 permille is out of range even from a flag");
        assert!(matches!(error, ConfigError::Invalid { .. }));
    }

    #[test]
    fn discover_from_skips_absent_files_and_returns_the_default() {
        let missing = std::path::Path::new("/nonexistent/monomyth-config-test/missing.toml");
        let config = ConfigResolver::discover_from(None, None, Some(missing))
            .expect("a missing file is skipped, not an error")
            .resolve()
            .expect("defaults validate");
        assert_eq!(*config.generation.fork_chance_permille.get(), 500);
    }

    #[test]
    fn discover_from_merges_the_project_layer() {
        let project = write_temp(
            "merges-project",
            "[generation]\nfork_chance_permille = 123\n",
        );
        let config = ConfigResolver::discover_from(None, None, Some(&project))
            .expect("valid file parses")
            .resolve()
            .expect("in-range file validates");
        assert_eq!(*config.generation.fork_chance_permille.get(), 123);
        assert_eq!(
            config.generation.fork_chance_permille.source(),
            LayerSource::ProjectOverride
        );
        std::fs::remove_file(&project).ok();
    }

    #[test]
    fn discover_from_lets_a_higher_layer_win_regardless_of_call_order() {
        let user = write_temp("order-user", "[generation]\nfork_chance_permille = 200\n");
        let project = write_temp(
            "order-project",
            "[generation]\nfork_chance_permille = 300\n",
        );
        // User outranks project (per ADR-0015), so the user value must win even
        // though the project file is merged last.
        let config = ConfigResolver::discover_from(None, Some(&user), Some(&project))
            .expect("both files parse")
            .resolve()
            .expect("in-range");
        assert_eq!(*config.generation.fork_chance_permille.get(), 200);
        assert_eq!(
            config.generation.fork_chance_permille.source(),
            LayerSource::UserOverride
        );
        std::fs::remove_file(&user).ok();
        std::fs::remove_file(&project).ok();
    }

    #[test]
    fn discover_from_surfaces_a_parse_error() {
        let bad = write_temp("parse-error", "[generation]\nfork_chance_permile = 1\n");
        let error = ConfigResolver::discover_from(None, None, Some(&bad))
            .expect_err("a misspelled key is a hard parse error");
        assert!(matches!(error, ConfigError::Parse { .. }));
        std::fs::remove_file(&bad).ok();
    }
}
