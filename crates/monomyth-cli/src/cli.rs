//! The command-line surface: clap derive types only, no logic.
//!
//! Parsing lives here; every handler lives in [`crate::commands`] and
//! [`crate::play`] so the real work is unit-testable without spawning a process.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

/// Default on-disk path for the knowledge vector store.
const DEFAULT_DB: &str = "./monomyth.db";
/// Default number of passages a `retrieve` returns.
const DEFAULT_TOP_K: u32 = 5;
/// Default directory holding the benchmark registry (`index.json`) and fixtures.
const DEFAULT_BENCHMARKS_DIR: &str = "./artifacts/benchmarks";
/// Default extraction strategy for `eval`.
const DEFAULT_EXTRACTOR: &str = "rag-softmax";
/// Default directory a drafted candidate law is written into for human review.
///
/// Deliberately outside `artifacts/`: a candidate here is PRE-REVIEW and must
/// never be mistaken for a committed, ship-safe law artifact (ADR-0016).
const DEFAULT_CANDIDATES_DIR: &str = "./synthesis/candidates";

/// monomyth: an adventure generation engine, composed into a playable text slice.
#[derive(Debug, Parser)]
#[command(name = "monomyth", version, about)]
pub(crate) struct Cli {
    /// `provider/model` routing string for the LLM content pass (`gen --fill`)
    /// and the `compose` pipeline. Overrides the configured `models.content` /
    /// `models.compose` default for whichever subcommand runs.
    #[arg(long, global = true)]
    pub(crate) model: Option<String>,

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

    /// Ingest a declared source's text into the knowledge store.
    ///
    /// Ship-namespace by default (surfaceable). Pass `--reference` to ingest a
    /// reference-namespace source into the priors-only reference collection
    /// (never surfaced verbatim); the ingest gate refuses a source whose ledger
    /// namespace does not match the chosen path.
    Ingest {
        /// Declared source id (must exist in the ledger and match the namespace).
        source: String,

        /// Inline text to ingest.
        #[arg(long, group = "input")]
        text: Option<String>,

        /// Path to a file whose contents are ingested.
        #[arg(long, group = "input")]
        file: Option<PathBuf>,

        /// Ingest into the reference collection (priors only, never surfaced)
        /// instead of the surfaceable ship collection.
        #[arg(long)]
        reference: bool,

        /// Optional human-readable title recorded with the document.
        #[arg(long)]
        title: Option<String>,

        /// Optional source URI (e.g. the exact URL the text was retrieved from),
        /// carried as provenance (ADR-0005).
        #[arg(long)]
        url: Option<String>,
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

    /// Export PD-gated text↔scored-structure fine-tune pairs for a fixture (JSONL).
    FinetuneExport {
        /// Benchmark fixture id from the registry (e.g. `odyssey_campbell_macro`).
        #[arg(long)]
        work: String,

        /// Directory holding the benchmark registry (`index.json`) and fixtures.
        #[arg(long, default_value = DEFAULT_BENCHMARKS_DIR)]
        benchmarks_dir: PathBuf,

        /// Write the JSONL here instead of to stdout.
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Draft a pre-review candidate law artifact from reference-namespace priors (ADR-0016).
    Synthesize {
        #[command(subcommand)]
        command: SynthesizeCommand,
    },

    /// Compose long-form adventure prose for a seeded world through the Plan→Draft→Revise→Assemble pipeline.
    Compose {
        /// Seed for the deterministic procedural world the prose narrates.
        #[arg(long)]
        seed: u64,

        /// Write the output here instead of stdout.
        #[arg(long)]
        out: Option<PathBuf>,

        /// Emit the full `LongFormDoc` as JSON (sections + prose) instead of just the prose text.
        #[arg(long)]
        json: bool,
    },
}

/// `synthesize` subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum SynthesizeCommand {
    /// Draft one candidate law and write it to synthesis/candidates/ for human review.
    Law {
        /// Machine-readable law id to stamp (e.g. `"three_act"`). Never seen by the model.
        #[arg(long)]
        law: String,

        /// Corpus domain (e.g. "myth", "folklore").
        #[arg(long)]
        domain: String,

        /// Retrieval query issued against the reference collection.
        #[arg(long)]
        query: String,

        /// Reference passages retrieved per coverage query. Overrides the
        /// configured `synthesis.per_query_top_k` default when set.
        #[arg(long)]
        top_k: Option<u32>,

        /// provider/model routing string for the synthesis LLM. Overrides the
        /// configured `models.synthesis` default when set.
        #[arg(long)]
        model: Option<String>,

        /// Seed coverage sub-queries from a framework taxonomy (e.g.
        /// "campbell"), so initial retrieval spans the whole arc. Opt-in;
        /// omit for a domain with no coverage framework.
        #[arg(long)]
        coverage_framework: Option<String>,

        /// Directory to write the candidate into (must NOT be under artifacts/).
        #[arg(long, default_value = DEFAULT_CANDIDATES_DIR)]
        out: PathBuf,
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

    /// Download reference/unverified sources into the reference/ inspect
    /// area for licensing review (never ingested into the ship corpus).
    Inspect {
        /// Restrict to a single declared source id.
        #[arg(long)]
        source: Option<String>,

        /// Cap the number of works fetched per source.
        #[arg(long)]
        limit: Option<usize>,
    },
}
