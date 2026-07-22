//! The `extract` subcommand: derive a minimal world structure directly from
//! raw source text via `monomyth-extract`'s [`MinimalStructureExtractor`]
//! (ADR-0018 Phase 3).
//!
//! This module wires the extractor into the binary only — it reads `--input`,
//! resolves the LLM model exactly as `gen --fill`/`compose` do (config +
//! `--model` override, routed through [`ModelRole::Content`]), optionally
//! biases the prompt with `--genre`, and writes the resulting [`World`] to
//! stdout or `--out`, mirroring `gen`'s own serialization. The extraction
//! logic itself lives entirely in `monomyth-extract`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use monomyth_config::{ConfigResolver, ModelRole, RuntimeOverrides};
use monomyth_contracts::StructureExtractor as _;
use monomyth_extract::MinimalStructureExtractor;
use monomyth_genre::{GenreKind, GenreProfile};
use monomyth_llm::{BackendOptions, Llm};

/// Resolve the `--genre` flag into a [`GenreProfile`], or `None` when omitted.
///
/// Pure and unit-testable: [`GenreKind::parse`] already falls back to
/// [`GenreKind::Myth`] for an unrecognized name (mirroring `genre.name` in
/// config), so an absent flag and an unrecognized one both end up framing the
/// prompt identically to [`MinimalStructureExtractor::new`]'s own default.
fn resolve_genre_profile(genre: Option<&str>) -> Option<GenreProfile> {
    genre.map(|name| GenreProfile {
        kind: GenreKind::parse(name),
        ..GenreProfile::default()
    })
}

/// Write a serialized world to `out` when given, otherwise to stdout —
/// mirrors `gen`'s own output handling exactly (`crate::commands::run_gen`).
///
/// # Errors
///
/// Fails if `out` is given and the file cannot be written.
fn write_world_output(serialized: &str, out: Option<&Path>) -> Result<()> {
    match out {
        Some(path) => {
            std::fs::write(path, serialized)
                .with_context(|| format!("writing world to {}", path.display()))?;
            println!("World written to {}", path.display());
        }
        None => println!("{serialized}"),
    }
    Ok(())
}

/// Handle `extract`: derive a minimal [`World`](monomyth_core::World) from
/// `input`'s text via one structured LLM call.
///
/// The input file is read before any configuration resolution or network
/// setup, so a bad `--input` path fails fast without touching the LLM. Uses
/// the real backend (via [`Llm::from_env_with_options`]) — this command is a
/// live run, not an offline test path; `monomyth-extract`'s own hermetic
/// cassette tests cover [`MinimalStructureExtractor::extract_structure`]
/// itself offline.
///
/// # Errors
///
/// Fails if `input` cannot be read, configuration resolution fails, the LLM
/// cannot be initialized from the environment, extraction fails, or
/// serialization/writing the output file fails.
pub(crate) async fn run_extract(
    input: PathBuf,
    out: Option<PathBuf>,
    genre: Option<String>,
    model: Option<String>,
) -> Result<()> {
    let text = std::fs::read_to_string(&input)
        .with_context(|| format!("reading input file {}", input.display()))?;

    let config = ConfigResolver::discover()
        .context("resolving configuration")?
        .with_runtime(RuntimeOverrides {
            content_model: model,
            ..RuntimeOverrides::default()
        })
        .resolve()
        .context("validating configuration")?;

    let content_model = config.model_for(ModelRole::Content).to_owned();
    let llm = Llm::from_env_with_options(&content_model, BackendOptions::default())
        .context("initializing the LLM from the environment")?;

    let genre_profile = resolve_genre_profile(genre.as_deref());
    let extractor = match &genre_profile {
        Some(profile) => MinimalStructureExtractor::with_genre(&llm, &content_model, profile),
        None => MinimalStructureExtractor::new(&llm, &content_model),
    };

    let world = extractor
        .extract_structure(&text)
        .await
        .context("extracting structure from the input text")?;

    let serialized =
        serde_json::to_string_pretty(&world).context("serializing the extracted world")?;
    write_world_output(&serialized, out.as_deref())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use monomyth_genre::GenreKind;

    use super::{resolve_genre_profile, run_extract, write_world_output};

    #[test]
    fn resolve_genre_profile_should_return_none_when_no_genre_given() {
        assert_eq!(resolve_genre_profile(None), None);
    }

    #[test]
    fn resolve_genre_profile_should_parse_a_recognized_genre_case_insensitively() {
        let profile = resolve_genre_profile(Some("Detective")).expect("a genre was given");
        assert_eq!(profile.kind, GenreKind::Detective);
    }

    #[test]
    fn resolve_genre_profile_should_fall_back_to_myth_for_an_unrecognized_genre() {
        let profile = resolve_genre_profile(Some("space-opera")).expect("a genre was given");
        assert_eq!(profile.kind, GenreKind::Myth);
    }

    #[test]
    fn write_world_output_should_write_to_the_given_file() {
        let temp_dir = tempfile::tempdir().expect("tempdir creates");
        let out_path = temp_dir.path().join("world.json");

        write_world_output("{\"fake\":\"world\"}", Some(&out_path))
            .expect("writing to an out path succeeds");

        let written = std::fs::read_to_string(&out_path).expect("output file reads back");
        assert_eq!(written, "{\"fake\":\"world\"}");
    }

    #[test]
    fn write_world_output_should_succeed_with_no_out_path() {
        write_world_output("{\"fake\":\"world\"}", None)
            .expect("writing with no out path prints to stdout and succeeds");
    }

    #[tokio::test]
    async fn run_extract_should_fail_fast_on_a_missing_input_file_before_touching_the_network() {
        let error = run_extract(
            Path::new("/nonexistent/does-not-exist.txt").to_path_buf(),
            None,
            None,
            None,
        )
        .await
        .expect_err("a missing input file must be rejected");

        assert!(
            error.to_string().contains("reading input file"),
            "error must name the input file read failure, got: {error}"
        );
    }
}
