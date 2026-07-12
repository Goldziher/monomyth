//! [`ConfigResolver`]: build a [`MonomythConfig`] by folding the layered sources.
//!
//! Resolution starts from the compiled [`MonomythConfig::default`]
//! ([`LayerSource::SystemDefault`]) and folds in, in precedence order, an optional
//! deployment file, the per-user file, the project file, and finally any runtime
//! (CLI) overrides. Missing files are skipped silently; a present-but-malformed
//! file is a hard [`ConfigError`].

use std::path::Path;

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
    /// [`MonomythConfig::default`] table.
    #[must_use]
    pub fn defaults() -> Self {
        Self {
            config: MonomythConfig::default(),
        }
    }

    /// Seed with defaults, then fold in the deployment, user, and project files
    /// that exist. A missing file is skipped; call order does not affect the
    /// result because precedence is fixed by [`LayerSource`] rank.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Read`] if a discovered file exists but cannot be
    /// read, or [`ConfigError::Parse`] if it is not valid TOML for the schema.
    pub fn discover() -> Result<Self, ConfigError> {
        let mut resolver = Self::defaults();

        if let Some(dir) = std::env::var_os(DEPLOYMENT_CONFIG_DIR_ENV) {
            let path = Path::new(&dir).join(PROJECT_CONFIG_FILE);
            resolver.merge_file(&path, LayerSource::DeploymentDefault)?;
        }
        if let Some(base) = dirs::config_dir() {
            let path = base.join(USER_CONFIG_SUBPATH);
            resolver.merge_file(&path, LayerSource::UserOverride)?;
        }
        resolver.merge_file(Path::new(PROJECT_CONFIG_FILE), LayerSource::ProjectOverride)?;

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
        self.config.synthesis.per_query_top_k.override_with(
            overrides.synthesis_per_query_top_k,
            LayerSource::RuntimeOverride,
        );
        self
    }

    /// The fully-resolved configuration.
    #[must_use]
    pub fn resolve(self) -> MonomythConfig {
        self.config
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
    use super::{ConfigResolver, RuntimeOverrides};
    use crate::layered::LayerSource;

    #[test]
    fn defaults_resolve_the_system_default() {
        let config = ConfigResolver::defaults().resolve();
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
            .resolve();
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
            .resolve();
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
            .resolve();
        assert_eq!(config.model_for(ModelRole::Content), "openai/gpt-5");
        assert_eq!(
            config.models.content.source(),
            LayerSource::RuntimeOverride,
            "the flag advances the content model to the runtime layer",
        );
        assert_eq!(config.models.synthesis.source(), LayerSource::SystemDefault);
    }
}
