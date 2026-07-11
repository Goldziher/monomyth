//! The `monomyth` binary: parse arguments and dispatch to a handler.
//!
//! This file is deliberately thin. All logic lives in [`commands`] and [`play`]
//! so it stays unit-testable without spawning a process or hitting a provider.

use anyhow::Result;
use clap::Parser;

use crate::cli::{Cli, Command, CorpusCommand};

mod cli;
mod commands;
mod evaluate;
mod play;

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    match cli.command {
        Command::Gen { seed, fill, out } => {
            commands::run_gen(seed, fill, out, &cli.model, &cli.db).await
        }
        Command::Play { world, seed } => {
            let world = commands::load_play_world(world.as_deref(), seed)?;
            play::run_play(world)
        }
        Command::Edit { world, script, out } => commands::run_edit(&world, &script, out),
        Command::Ingest { source, text, file } => {
            let content = commands::resolve_text(text, file.as_deref())?;
            commands::run_ingest(&source, content, &cli.db).await
        }
        Command::Retrieve { query, top_k } => commands::run_retrieve(&query, top_k, &cli.db).await,
        Command::Corpus { command } => match command {
            CorpusCommand::Build { source, limit } => {
                commands::run_corpus_build(source, limit, &cli.db).await
            }
            CorpusCommand::Audit {} => commands::run_corpus_audit(&cli.db).await,
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
    }
}
