//! The `compose` subcommand: narrate a seeded world into long-form prose
//! through `monomyth-compose`'s Plan→Draft→Revise→Assemble pipeline.
//!
//! This module wires the pipeline into the binary only — it builds a world
//! from a seed exactly as `gen` does (via [`crate::commands::generate_world`]),
//! resolves the `[compose]` config knobs into the pipeline's own
//! [`ComposeSettings`], and writes the resulting prose (or, with `--json`, the
//! full [`monomyth_compose::LongFormDoc`]) to stdout or a file. The pipeline
//! logic itself lives entirely in `monomyth-compose`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use monomyth_compose::{ComposeSettings, compose};
use monomyth_config::{ConfigResolver, ModelRole, RuntimeOverrides};
use monomyth_knowledge::Knowledge;
use monomyth_llm::{BackendOptions, Llm};

use crate::commands::generate_world;

/// Handle `compose`: generate a world's structure from `seed`, then narrate it
/// end to end into a [`monomyth_compose::LongFormDoc`].
///
/// Uses the real Gemini backend (via [`Llm::from_env_with_options`]) and the
/// production knowledge store (via [`Knowledge::open`], local ONNX
/// `CoreEmbedder` + sqlite) — this command is a live run, not an offline test
/// path. Output is the assembled prose by default, or the full document as
/// JSON with `--json`; written to `out` when given, otherwise printed to
/// stdout.
///
/// # Errors
///
/// Fails if configuration resolution, world generation, LLM/knowledge
/// initialization, composition, serialization, or writing the output file
/// fails.
pub(crate) async fn run_compose(
    seed: u64,
    out: Option<PathBuf>,
    json: bool,
    model: Option<String>,
    db: &Path,
) -> Result<()> {
    let config = ConfigResolver::discover()
        .context("resolving configuration")?
        .with_runtime(RuntimeOverrides {
            compose_model: model,
            ..RuntimeOverrides::default()
        })
        .resolve()
        .context("validating configuration")?;

    let world = generate_world(seed, &crate::commands::generation_config(&config))?;

    let settings = ComposeSettings {
        max_turns: *config.compose.max_turns.get(),
        grounding_top_k: *config.compose.grounding_top_k.get(),
        revise_threshold: *config.compose.revise_threshold.get(),
        max_revise_iterations: *config.compose.max_revise_iterations.get(),
    };

    let model = config.model_for(ModelRole::Compose).to_owned();
    let llm = Llm::from_env_with_options(&model, BackendOptions::default())
        .context("initializing the compose LLM from the environment")?;
    let knowledge = Knowledge::open(db)
        .await
        .context("opening the knowledge store")?;

    let doc = compose(&llm, &knowledge, &world, &settings)
        .await
        .context("composing long-form prose")?;

    let output = if json {
        serde_json::to_string_pretty(&doc).context("serializing the composed document")?
    } else {
        doc.prose().to_owned()
    };

    match out {
        Some(path) => {
            std::fs::write(&path, &output)
                .with_context(|| format!("writing composed output to {}", path.display()))?;
            println!("Composed output written to {}", path.display());
        }
        None => println!("{output}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn default_config_compose_knobs_match_compose_settings_default() {
        use monomyth_compose::ComposeSettings;
        use monomyth_config::ConfigResolver;

        let config = ConfigResolver::defaults()
            .resolve()
            .expect("defaults validate");
        let settings_default = ComposeSettings::default();

        assert_eq!(*config.compose.max_turns.get(), settings_default.max_turns);
        assert_eq!(
            *config.compose.grounding_top_k.get(),
            settings_default.grounding_top_k
        );
        assert!(
            (*config.compose.revise_threshold.get() - settings_default.revise_threshold).abs()
                < f64::EPSILON
        );
        assert_eq!(
            *config.compose.max_revise_iterations.get(),
            settings_default.max_revise_iterations
        );
    }
}
