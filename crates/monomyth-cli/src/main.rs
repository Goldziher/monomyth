//! The `monomyth` binary: parse arguments and dispatch to a handler.
//!
//! This file is deliberately thin. All logic lives in [`commands`] and [`play`]
//! so it stays unit-testable without spawning a process or hitting a provider.

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command, CorpusCommand, SynthesizeCommand};

mod cli;
mod commands;
mod compose;
mod evaluate;
mod play;
mod synthesize;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();
    let cli = Cli::parse();

    match cli.command {
        Command::Gen { seed, fill, out } => {
            commands::run_gen(seed, fill, out, cli.model, &cli.db).await
        }
        Command::Play { world, seed } => {
            let config = commands::resolve_config()?;
            let renderer = commands::build_renderer(&config);
            let generation_config = commands::generation_config(&config);
            let world = commands::load_play_world(world.as_deref(), seed, &generation_config)?;
            play::run_play(world, renderer.as_ref())
        }
        Command::Edit { world, script, out } => {
            let config = commands::resolve_config()?;
            let renderer = commands::build_renderer(&config);
            commands::run_edit(&world, &script, out, renderer.as_ref())
        }
        Command::Ingest {
            source,
            text,
            file,
            reference,
            title,
            url,
        } => {
            let content = commands::resolve_text(text, file.as_deref())?;
            commands::run_ingest(&source, content, reference, title, url, &cli.db).await
        }
        Command::Retrieve { query, top_k } => commands::run_retrieve(&query, top_k, &cli.db).await,
        Command::Corpus { command } => match command {
            CorpusCommand::Build { source, limit } => {
                commands::run_corpus_build(source, limit, &cli.db).await
            }
            CorpusCommand::Audit {} => commands::run_corpus_audit(&cli.db).await,
            CorpusCommand::Inspect { source, limit } => {
                commands::run_corpus_inspect(source, limit, &cli.db).await
            }
        },
        Command::Eval {
            work,
            extractor,
            benchmarks_dir,
            out,
        } => evaluate::run_eval(&work, &extractor, &benchmarks_dir, out, &cli.db).await,
        Command::FinetuneExport {
            work,
            benchmarks_dir,
            out,
        } => evaluate::run_finetune_export(&work, &benchmarks_dir, out),
        Command::Synthesize { command } => match command {
            SynthesizeCommand::Law {
                law,
                domain,
                query,
                top_k,
                model,
                coverage_framework,
                out,
            } => {
                synthesize::run_synthesize_law(
                    synthesize::SynthesizeLawArgs {
                        law,
                        domain,
                        query,
                        top_k,
                        model,
                        coverage_framework,
                        out,
                    },
                    &cli.db,
                )
                .await
            }
        },
        Command::Compose { seed, out, json } => {
            compose::run_compose(seed, out, json, cli.model, &cli.db).await
        }
    }
}

/// Install a `tracing` subscriber that emits the LLM/generation spans and events
/// (`monomyth-llm` timing, token usage, retry counts) to stderr.
///
/// Filtering is driven by `RUST_LOG` (e.g. `RUST_LOG=monomyth_llm=info`); with no
/// `RUST_LOG` set it defaults to `warn`, so normal runs stay quiet while the
/// machinery is one env var away. Failing to install (e.g. a subscriber already
/// set in a test harness) is ignored — logging is best-effort, never fatal.
fn init_tracing() {
    use tracing_subscriber::EnvFilter;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}
