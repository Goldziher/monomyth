//! The command-line surface: clap derive types only, no logic.
//!
//! Parsing lives here; every handler lives in [`crate::commands`] and
//! [`crate::play`] so the real work is unit-testable without spawning a process.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Default `provider/model` routing string for the content pass.
const DEFAULT_MODEL: &str = "openai/gpt-4o-mini";
/// Default on-disk path for the knowledge vector store.
const DEFAULT_DB: &str = "./monomyth.db";
/// Default number of passages a `retrieve` returns.
const DEFAULT_TOP_K: u32 = 5;
/// Default directory holding the benchmark registry (`index.json`) and fixtures.
const DEFAULT_BENCHMARKS_DIR: &str = "./artifacts/benchmarks";
/// Default extraction strategy for `eval`.
const DEFAULT_EXTRACTOR: &str = "rag-softmax";

/// monomyth: an adventure generation engine, composed into a playable text slice.
#[derive(Debug, Parser)]
#[command(name = "monomyth", version, about)]
pub(crate) struct Cli {
    /// `provider/model` routing string passed to the LLM content pass.
    #[arg(long, global = true, default_value = DEFAULT_MODEL)]
    pub(crate) model: String,

    /// Path to the knowledge vector store used by ingest, retrieve, and `--fill`.
    #[arg(long, global = true, default_value = DEFAULT_DB)]
    pub(crate) db: PathBuf,

    /// The subcommand to run.
    #[command(subcommand)]
    pub(crate) command: Command,
}

/// The top-level subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum Command {
    /// Generate a world from a seed, render it, and emit its serialized form.
    Gen {
        /// Seed for the deterministic procedural pass.
        #[arg(long)]
        seed: u64,

        /// Also run the (non-deterministic) LLM content pass to fill prose.
        #[arg(long)]
        fill: bool,

        /// Write the serialized world here instead of to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Play a world interactively: load one from disk or generate from a seed.
    Play {
        /// Load a previously serialized world from this path.
        #[arg(long)]
        world: Option<PathBuf>,

        /// Generate a fresh world structure from this seed.
        #[arg(long)]
        seed: Option<u64>,
    },

    /// Apply a JSON edit script to a world's narrative structure and re-validate.
    Edit {
        /// Load the world to edit from this path.
        #[arg(long)]
        world: PathBuf,

        /// Path to a JSON array of narrative edit operations to apply.
        #[arg(long)]
        script: PathBuf,

        /// Write the edited world here instead of to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Ingest a ship-namespace source's text into the knowledge store.
    Ingest {
        /// Declared source id (must exist in the ledger and be ship-namespace).
        source: String,

        /// Inline text to ingest.
        #[arg(long, group = "input")]
        text: Option<String>,

        /// Path to a file whose contents are ingested.
        #[arg(long, group = "input")]
        file: Option<PathBuf>,
    },

    /// Retrieve surfaceable passages matching a query.
    Retrieve {
        /// The query text.
        query: String,

        /// Maximum number of passages to return.
        #[arg(long, default_value_t = DEFAULT_TOP_K)]
        top_k: u32,
    },

    /// Manage the ship-safe corpus: fetch, normalize, and ingest declared sources.
    Corpus {
        #[command(subcommand)]
        command: CorpusCommand,
    },

    /// Score an extractor against a ground-truth benchmark fixture (ADR-0023).
    Eval {
        /// Benchmark fixture id from the registry (e.g. `odyssey_campbell_macro`).
        #[arg(long)]
        work: String,

        /// Extraction strategy to score. Only `rag-softmax` is available today.
        #[arg(long, default_value = DEFAULT_EXTRACTOR)]
        extractor: String,

        /// Directory holding the benchmark registry (`index.json`) and fixtures.
        #[arg(long, default_value = DEFAULT_BENCHMARKS_DIR)]
        benchmarks_dir: PathBuf,

        /// Write the JSON report here instead of to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

/// Corpus acquisition subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum CorpusCommand {
    /// Fetch, normalize, and ingest ship-safe sources declared in the ledger.
    Build {
        /// Restrict to a single declared source id.
        #[arg(long)]
        source: Option<String>,

        /// Cap the number of works fetched and ingested per source.
        #[arg(long)]
        limit: Option<usize>,
    },

    /// Audit stored document metadata against the license ledger (ADR-0005's
    /// third enforcement point, after ingest and retrieval).
    Audit {},
}
