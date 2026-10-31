//! The `synthesize law` and `synthesize promote` subcommands (ADR-0016 Phases
//! 2d/2e).
//!
//! `monomyth-synthesis::draft_law` produces a [`monomyth_synthesis::DraftedLaw`]
//! whose `artifact.synthesis.reviewed_by` is always empty — structurally
//! incapable of loading via [`monomyth_frameworks::load_law`] until a human
//! reviews it. `synthesize law`'s whole job is to make that review boundary
//! impossible to miss: candidates are written to a gitignored `synthesis/`
//! directory, never to `artifacts/`, and every run ends with a REVIEW-REQUIRED
//! banner on stderr pointing at `synthesize promote`.
//!
//! `synthesize promote` is the other half of that boundary: it stamps a
//! human-supplied reviewer identity onto a candidate, re-runs the anti-leak
//! gate (a human edit could reintroduce verbatim wording), validates the
//! result via [`monomyth_frameworks::load_law`], and only then writes into
//! `artifacts/laws/` — the one path in this module that is allowed to.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use monomyth_config::{ConfigResolver, ModelRole, RuntimeOverrides};
use monomyth_frameworks::{LawArtifact, load_law};
use monomyth_knowledge::Knowledge;
use monomyth_llm::{BackendOptions, Llm};
use monomyth_synthesis::{
    CoverageFramework, DraftRequest, DraftedLaw, JudgeVerdict, LoopConfig, PreScore,
    coverage_sub_queries, draft_law, verify_no_verbatim,
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::macros::format_description;

/// The path segment that marks a directory as the committed, human-reviewed
/// home for law artifacts. A candidate must never be written under it.
const ARTIFACTS_SEGMENT: &str = "artifacts";

/// Check whether `out` would place a written candidate under an `artifacts`
/// directory, and return a refusal reason if so.
///
/// Purely lexical (a component scan for an `artifacts` segment): it needs no
/// filesystem access, so it can run before any network/db work and is
/// trivially unit-testable. This also means it catches a path like
/// `./artifacts/laws` or `foo/artifacts/x` even when neither exists yet.
fn rejects_forbidden_out_dir(out: &Path) -> Option<&'static str> {
    let has_artifacts_segment = out
        .components()
        .any(|component| component.as_os_str() == ARTIFACTS_SEGMENT);
    has_artifacts_segment.then_some(
        "refusing to write a candidate into artifacts/; that directory is the human-reviewed, \
         committed home — write to synthesis/candidates/ and promote after review",
    )
}

/// One retrieved passage's provenance, as recorded in the local review context
/// file. Deliberately narrower than [`monomyth_knowledge::Passage`]: only the
/// fields a reviewer needs to judge grounding quality.
#[derive(Debug, Serialize)]
struct ContextPassage {
    /// The declaring source id.
    source_id: String,
    /// Retrieval relevance score.
    score: f32,
    /// Length of the passage text, for a quick grounding-density glance.
    char_count: usize,
    /// The full passage text.
    ///
    /// Safe to include here even though it is reference-namespace (never
    /// surfaceable) material: this file is written only under the gitignored
    /// `synthesis/` directory for LOCAL human review and is never committed
    /// or shipped.
    text: String,
}

/// The local review-context sidecar written alongside a candidate artifact.
///
/// Records grounding provenance a reviewer needs to judge the candidate
/// against its sources, without polluting the artifact file itself.
#[derive(Debug, Serialize)]
struct ReviewContext {
    /// The machine-readable law id the candidate was drafted for.
    law_id: String,
    /// The `provider/model` routing string used for distillation.
    model: String,
    /// The ISO 8601 date the candidate was generated.
    generated: String,
    /// The retrieval query issued against the reference collection.
    query: String,
    /// The sha256 hex digest of the candidate's canonical JSON.
    candidate_sha256: String,
    /// The best-scoring candidate's weighted judge score (0-100).
    final_score: f64,
    /// How many judge/refine iterations ran before the loop stopped.
    iterations: u32,
    /// The judge's full verdict for the best candidate, when a judge call
    /// succeeded — its per-criterion scores, named missing phases, and
    /// improvement instructions, so the reviewer sees the machine's own
    /// assessment beside the sources.
    verdict: Option<JudgeVerdict>,
    /// A deterministic, no-LLM second opinion (coverage / ordering / grounding
    /// overlap) the reviewer can hold up against the judge's score. Present only
    /// when the run targeted a coverage framework.
    pre_score: Option<PreScore>,
    /// Names of items the model marked as not derivable from the grounding — an
    /// honest abstention the reviewer should scrutinize before promoting.
    abstained_phases: Vec<String>,
    /// Token usage for the distillation call, when the backend reported one.
    usage: Option<monomyth_llm::Usage>,
    /// The reference passages retrieved as grounding, in retrieval order.
    passages: Vec<ContextPassage>,
}

/// Build the REVIEW-REQUIRED banner text, spelling out the promotion steps.
///
/// A `fn` rather than inline `eprintln!` calls so a test can assert the key
/// phrases survive edits to this message.
fn review_required_banner(
    law_id: &str,
    artifact_path: &Path,
    context_path: &Path,
    final_score: f64,
    iterations: u32,
) -> String {
    format!(
        "REVIEW REQUIRED: wrote a PRE-REVIEW candidate for law {law_id:?}.\n\
         This candidate will NOT load (monomyth_frameworks::load_law refuses an empty \
         synthesis.reviewed_by) until a human reviews it.\n\
         \n\
         Judge score: {final_score:.0}/100 after {iterations} iteration(s)\n\
         Candidate artifact:  {}\n\
         Review context:      {}\n\
         \n\
         To promote this candidate:\n\
         1. Read the candidate and its .context.json; compare wording against the recorded \
         sources, and edit wording as needed — the model's authority was the abstract idea \
         only.\n\
         2. Run `monomyth synthesize promote {} --reviewed-by <your identity>` — it re-runs \
         the anti-leak gate, validates via monomyth_frameworks::load_law, and writes into \
         artifacts/laws/ (updating its index).\n",
        artifact_path.display(),
        context_path.display(),
        artifact_path.display(),
    )
}

/// Write a drafted candidate's artifact and review context into `out_dir`,
/// returning the artifact's path.
///
/// Writes two files: `<out_dir>/<law_id>.json` (the pre-review
/// [`monomyth_frameworks::LawArtifact`] itself — `reviewed_by` empty) and
/// `<out_dir>/<law_id>.context.json` (grounding provenance for the reviewer,
/// including passage text — safe only because this whole directory tree is
/// gitignored and local-only, never committed or shipped).
///
/// # Errors
///
/// Fails if `law_id` is not a safe path component ([`validate_law_slug`]), if
/// `out_dir` cannot be created, if either file cannot be serialized, or if
/// either file cannot be written.
fn write_candidate(
    drafted: &DraftedLaw,
    law_id: &str,
    query: &str,
    out_dir: &Path,
) -> Result<PathBuf> {
    // `law_id` here is an operator-typed `--law` CLI argument, not adversarial content
    // read from a file, so it is a lower-trust-boundary case than `run_synthesize_promote`'s
    // `artifact.law` — but validating it before building any path is cheap, keeps every
    // invariant, and a mistyped `--law` value fails fast with a clear message instead of
    // writing outside `out_dir`.
    validate_law_slug(law_id).with_context(|| format!("--law {law_id:?} is not a safe law id"))?;

    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("creating candidate directory {}", out_dir.display()))?;

    let artifact_path = out_dir.join(format!("{law_id}.json"));
    let artifact_json = serde_json::to_string_pretty(&drafted.artifact)
        .context("serializing the candidate law artifact")?;
    std::fs::write(&artifact_path, artifact_json)
        .with_context(|| format!("writing candidate artifact {}", artifact_path.display()))?;

    let context_path = out_dir.join(format!("{law_id}.context.json"));
    let review_context = ReviewContext {
        law_id: law_id.to_owned(),
        model: drafted.artifact.synthesis.model.clone(),
        generated: drafted.artifact.synthesis.generated.clone(),
        query: query.to_owned(),
        candidate_sha256: drafted.candidate_sha256.clone(),
        final_score: drafted.final_score,
        iterations: drafted.iterations,
        verdict: drafted.verdict.clone(),
        pre_score: drafted.pre_score.clone(),
        abstained_phases: drafted
            .candidate
            .items
            .iter()
            .filter(|item| !item.derivable)
            .map(|item| item.name.clone())
            .collect(),
        usage: drafted.usage.clone(),
        passages: drafted
            .passages
            .iter()
            .map(|passage| ContextPassage {
                source_id: passage.source_id.clone(),
                score: passage.score,
                char_count: passage.text.chars().count(),
                text: passage.text.clone(),
            })
            .collect(),
    };
    let context_json = serde_json::to_string_pretty(&review_context)
        .context("serializing the candidate review context")?;
    std::fs::write(&context_path, context_json)
        .with_context(|| format!("writing review context {}", context_path.display()))?;

    Ok(artifact_path)
}

/// The resolved arguments for [`run_synthesize_law`], bundled so the handler
/// stays under clippy's argument-count limit and the composition root
/// ([`crate::main`]) builds one value from the parsed subcommand.
pub(crate) struct SynthesizeLawArgs {
    /// Machine-readable law id to stamp; never seen by the model.
    pub(crate) law: String,
    /// Corpus domain the law belongs to (e.g. `"myth"`).
    pub(crate) domain: String,
    /// Seed retrieval query against the reference collection.
    pub(crate) query: String,
    /// Reference passages retrieved per coverage query (the loop's
    /// `per_query_top_k`). `None` resolves the configured default; `Some` is a
    /// runtime override.
    pub(crate) top_k: Option<u32>,
    /// `provider/model` routing string for the synthesis LLM. `None` resolves the
    /// configured `models.synthesis` default; `Some` is a runtime override.
    pub(crate) model: Option<String>,
    /// Optional framework whose taxonomy seeds coverage sub-queries.
    pub(crate) coverage_framework: Option<String>,
    /// Directory to write the candidate into (must not be under `artifacts/`).
    pub(crate) out: PathBuf,
}

/// Handle `synthesize law`: draft a pre-review candidate law artifact and
/// write it to `out` for human review.
///
/// The forbidden-directory guard ([`rejects_forbidden_out_dir`]) runs first,
/// before any network or database work, so a mistaken `--out artifacts/laws`
/// fails fast rather than after spending a retrieval + LLM call.
///
/// # Errors
///
/// Fails if `out` resolves under `artifacts/`, if the LLM or knowledge store
/// cannot be initialized, if today's date cannot be formatted, if
/// [`monomyth_synthesis::draft_law`] fails (see [`monomyth_synthesis::SynthesisError`]),
/// or if writing the candidate files fails.
pub(crate) async fn run_synthesize_law(args: SynthesizeLawArgs, db: &Path) -> Result<()> {
    let SynthesizeLawArgs {
        law,
        domain,
        query,
        top_k,
        model,
        coverage_framework,
        out,
    } = args;

    if let Some(reason) = rejects_forbidden_out_dir(&out) {
        bail!("{reason}");
    }

    // Resolve coverage enrichment before any network/db work, mirroring the ~keep
    // forbidden-directory guard's fail-fast: a typo'd framework name should ~keep
    // error immediately, not after spending a retrieval + LLM call. ~keep
    let sub_queries = match coverage_framework.as_deref() {
        None => Vec::new(),
        Some(key) => {
            let framework = CoverageFramework::from_key(key).with_context(|| {
                format!(
                    "unknown --coverage-framework {key:?}; known: {}",
                    CoverageFramework::known_keys().join(", ")
                )
            })?;
            coverage_sub_queries(framework)
        }
    };

    let config = ConfigResolver::discover()
        .context("resolving configuration")?
        .with_runtime(RuntimeOverrides {
            synthesis_model: model,
            synthesis_per_query_top_k: top_k,
            ..RuntimeOverrides::default()
        })
        .resolve()
        .context("validating configuration")?;
    let model = config.model_for(ModelRole::Synthesis).to_owned();

    let llm = Llm::from_env_with_options(&model, BackendOptions::default())
        .context("initializing the synthesis LLM from the environment")?;
    let knowledge = Knowledge::open(db)
        .await
        .context("opening the knowledge store")?;

    let date_format = format_description!("[year]-[month]-[day]");
    let generated = OffsetDateTime::now_utc()
        .format(&date_format)
        .context("formatting today's date")?;

    let request = DraftRequest {
        law_id: law.clone(),
        domain,
        query,
        sub_queries,
        model,
        generated,
        loop_config: LoopConfig {
            max_iterations: *config.synthesis.max_iterations.get(),
            per_query_top_k: *config.synthesis.per_query_top_k.get(),
            max_grounding: *config.synthesis.max_grounding.get(),
            ..LoopConfig::default()
        },
    };
    let drafted = draft_law(&knowledge, &llm, &request)
        .await
        .context("drafting the candidate law")?;

    let artifact_path = write_candidate(&drafted, &law, &request.query, &out)?;
    let context_path = out.join(format!("{law}.context.json"));

    eprintln!(
        "{}",
        review_required_banner(
            &law,
            &artifact_path,
            &context_path,
            drafted.final_score,
            drafted.iterations,
        )
    );
    println!("{}", artifact_path.display());
    Ok(())
}

/// The subset of a written review-context sidecar's passages needed to
/// re-run the anti-leak gate at promotion time.
///
/// Deliberately narrower than [`ReviewContext`]/[`ContextPassage`] (which are
/// write-only, for `synthesize law`): unknown JSON fields are ignored on
/// deserialize, so this reads a real sidecar without requiring every
/// optional provenance field the writer emits (`verdict`, `pre_score`,
/// `usage`, ...).
#[derive(Debug, Deserialize)]
struct SidecarPassage {
    /// The declaring source id, named on an anti-leak hit.
    source_id: String,
    /// The full passage text the anti-leak gate re-checks the candidate against.
    text: String,
}

/// The subset of a written review-context sidecar needed at promotion time.
#[derive(Debug, Deserialize)]
struct SidecarReviewContext {
    /// The reference passages retrieved as grounding, in retrieval order.
    passages: Vec<SidecarPassage>,
}

/// One entry of `<laws-dir>/index.json`'s `laws` array.
#[derive(Debug, Deserialize, Serialize)]
struct LawIndexEntry {
    /// Machine-readable law id.
    law: String,
    /// Number of items in the law.
    count: usize,
    /// Shippability namespace (always `"ship"` for a valid promoted law).
    namespace: String,
    /// Licensing tier (always `"system"` for a valid promoted law).
    tier: String,
}

/// `<laws-dir>/index.json`'s shape, mirroring `artifacts/frameworks/index.json`.
#[derive(Debug, Deserialize, Serialize)]
struct LawIndex {
    /// Human-readable index title.
    index: String,
    /// Human-readable explanatory note.
    note: String,
    /// The registered laws, in registration order.
    laws: Vec<LawIndexEntry>,
}

/// Reject an empty or whitespace-only reviewer identity.
///
/// This is the human-review gate itself: promotion must name a real person
/// who reviewed the candidate's wording, not merely record that *a* value was
/// passed.
///
/// # Errors
///
/// Fails if `reviewed_by` is empty or contains only whitespace.
fn require_reviewer_identity(reviewed_by: &str) -> Result<()> {
    if reviewed_by.trim().is_empty() {
        bail!(
            "--reviewed-by must not be empty or whitespace-only; promotion requires a human \
             reviewer's identity"
        );
    }
    Ok(())
}

/// Derive a candidate artifact's review-context sidecar path: `<stem>.context.json`
/// beside the candidate itself, matching what `synthesize law` writes.
///
/// # Errors
///
/// Fails if `candidate_path` has no usable UTF-8 file stem.
fn sidecar_path_for(candidate_path: &Path) -> Result<PathBuf> {
    let stem = candidate_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| {
            format!(
                "candidate path {} has no usable file stem",
                candidate_path.display()
            )
        })?;
    let parent = candidate_path.parent().unwrap_or_else(|| Path::new(""));
    Ok(parent.join(format!("{stem}.context.json")))
}

/// Maximum length, in bytes, of a law slug (`artifact.law`). Bounds the
/// filename component built from it; 64 bytes comfortably fits every real law
/// id (e.g. `monomyth_macro_arc`) with headroom.
const MAX_LAW_SLUG_LEN: usize = 64;

/// Validate that `slug` is safe to interpolate into a filesystem path
/// component: non-empty, at most [`MAX_LAW_SLUG_LEN`] bytes, and composed
/// only of ASCII lowercase letters, digits, and underscores (`^[a-z0-9_]+$`).
///
/// `artifact.law` is reviewer-controlled free text read from a candidate JSON
/// file, not a value this process generated. [`run_synthesize_promote`] and
/// [`refuse_if_already_promoted`] both build filesystem paths by
/// interpolating it directly (`laws_dir.join(format!("{law}.json"))`);
/// without this check a crafted slug such as `"../frameworks/x"` or an
/// absolute path escapes `laws_dir` (CWE-22 path traversal). A byte-level
/// allowlist scan rather than a `regex` dependency: the language is trivial
/// enough that pulling in a regex engine for it is not worth it.
///
/// # Errors
///
/// Fails if `slug` is empty, exceeds [`MAX_LAW_SLUG_LEN`] bytes, or contains
/// any byte outside `[a-z0-9_]`.
fn validate_law_slug(slug: &str) -> Result<()> {
    if slug.is_empty() {
        bail!("law id must not be empty");
    }
    if slug.len() > MAX_LAW_SLUG_LEN {
        bail!(
            "law id {slug:?} is {} bytes, exceeding the {MAX_LAW_SLUG_LEN}-byte limit",
            slug.len()
        );
    }
    let is_valid_byte =
        |byte: u8| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_';
    if !slug.bytes().all(is_valid_byte) {
        bail!(
            "law id {slug:?} must match ^[a-z0-9_]+$ (lowercase ascii letters, digits, underscore \
             only)"
        );
    }
    Ok(())
}

/// Read and parse a candidate artifact as a [`LawArtifact`].
///
/// # Errors
///
/// Fails if `candidate_path` cannot be read or does not parse as a [`LawArtifact`].
fn read_candidate_artifact(candidate_path: &Path) -> Result<LawArtifact> {
    let json = std::fs::read_to_string(candidate_path)
        .with_context(|| format!("reading candidate artifact {}", candidate_path.display()))?;
    serde_json::from_str(&json).with_context(|| {
        format!(
            "parsing candidate artifact {} as a law artifact",
            candidate_path.display()
        )
    })
}

/// Read and parse a candidate's review-context sidecar.
///
/// # Errors
///
/// Fails if `sidecar_path` cannot be read or does not parse as a [`SidecarReviewContext`].
fn read_sidecar_context(sidecar_path: &Path) -> Result<SidecarReviewContext> {
    let json = std::fs::read_to_string(sidecar_path)
        .with_context(|| format!("reading review context {}", sidecar_path.display()))?;
    serde_json::from_str(&json)
        .with_context(|| format!("parsing review context {}", sidecar_path.display()))
}

/// The `index` field of a freshly bootstrapped `index.json`, written the
/// first time `synthesize promote` runs against a `laws_dir` that has no
/// index yet.
const BOOTSTRAP_INDEX_TITLE: &str = "Synthesized laws";

/// The `note` field of a freshly bootstrapped `index.json`.
const BOOTSTRAP_INDEX_NOTE: &str = "Build-time-synthesized, human-reviewed structural laws (ADR-0016). Auto-created by \
     `synthesize promote`.";

/// Read `<laws-dir>/index.json`, treating a missing file as an empty index.
///
/// A missing `index.json` is not an error: it is exactly the state of a
/// brand-new `laws_dir` before its first promotion, and `synthesize promote`
/// must be able to bootstrap that first promotion rather than requiring some
/// other step to pre-create an empty index.
///
/// # Errors
///
/// Fails if `index_path` exists but cannot be read, or does not parse as a
/// [`LawIndex`].
fn read_laws_index(index_path: &Path) -> Result<LawIndex> {
    let json = match std::fs::read_to_string(index_path) {
        Ok(json) => json,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LawIndex {
                index: BOOTSTRAP_INDEX_TITLE.to_owned(),
                note: BOOTSTRAP_INDEX_NOTE.to_owned(),
                laws: Vec::new(),
            });
        }
        Err(error) => {
            return Err(error)
                .with_context(|| format!("reading laws index {}", index_path.display()));
        }
    };
    serde_json::from_str(&json)
        .with_context(|| format!("parsing laws index {}", index_path.display()))
}

/// Refuse promotion if `law_id` is already registered in `index` — the
/// authoritative record of what is promoted.
///
/// An on-disk `<law_id>.json` that exists but is *not* indexed is reported to
/// stderr as an unindexed-orphan warning rather than a hard block. Index
/// membership, not file existence, is authoritative: a law file can exist
/// without being indexed only if a prior promotion was interrupted between
/// writing the artifact and updating the index (or the file was placed there
/// by hand), and treating that as a permanent block would wedge recovery —
/// the file can never be indexed and can never be overwritten either.
///
/// # Errors
///
/// Fails if `law_id` is already registered in `index`.
fn refuse_if_already_promoted(laws_dir: &Path, law_id: &str, index: &LawIndex) -> Result<()> {
    if index.laws.iter().any(|entry| entry.law == law_id) {
        bail!(
            "law {law_id:?} is already registered in {}",
            laws_dir.join("index.json").display()
        );
    }
    let law_path = laws_dir.join(format!("{law_id}.json"));
    if law_path.exists() {
        eprintln!(
            "warning: {} exists but law {law_id:?} is not registered in the index; treating it \
             as an unindexed orphan (likely an interrupted prior promotion) and overwriting it",
            law_path.display()
        );
    }
    Ok(())
}

/// Write `contents` to `path` atomically: write to a sibling temporary file
/// in the same directory, then `rename` it over `path`. A crash or a
/// concurrent reader can therefore never observe a partially-written law
/// artifact or index — `rename` within a single filesystem is atomic, unlike
/// a direct [`std::fs::write`], which can leave a truncated file behind if
/// the process is interrupted mid-write.
///
/// # Errors
///
/// Fails if `path` has no parent directory, if the temporary file cannot be
/// written, or if the rename fails.
fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("{} has no usable UTF-8 file name", path.display()))?;
    let temp_path = parent.join(format!(".{file_name}.tmp-{}", std::process::id()));
    std::fs::write(&temp_path, contents)
        .with_context(|| format!("writing temporary file {}", temp_path.display()))?;
    std::fs::rename(&temp_path, path).with_context(|| {
        format!(
            "renaming {} into place at {}",
            temp_path.display(),
            path.display()
        )
    })
}

/// Collect every free-text field the anti-leak gate must re-check against the
/// recorded grounding: the artifact's `title` and `tier_note`, plus every
/// item's name and description.
///
/// `title` and `tier_note` are reviewer-editable free text exactly like an
/// item's `description` — a human edit to either during review can just as
/// easily reintroduce verbatim source wording, so omitting them from the gate
/// would leave a leak path open that the item-only check never covers.
fn candidate_leak_texts(artifact: &LawArtifact) -> Vec<&str> {
    let mut texts = vec![artifact.title.as_str(), artifact.tier_note.as_str()];
    texts.extend(
        artifact
            .items
            .iter()
            .flat_map(|item| [item.name.as_str(), item.description.as_str()]),
    );
    texts
}

/// Minimum whitespace-split word count a sidecar passage must have to produce
/// at least one shingle in the anti-leak gate.
///
/// Mirrors `monomyth_synthesis::antileak::SHINGLE_N` (currently 8), which is
/// private to that module: a passage shorter than the shingle width yields
/// zero shingles, so [`monomyth_synthesis::verify_no_verbatim`] silently finds
/// nothing to compare it against — the same neutered-gate failure mode as an
/// empty `passages` array. Update this constant if `SHINGLE_N` changes.
const MIN_SHINGLE_WORDS: usize = 8;

/// Collect `(source_id, text)` reference chunks from a sidecar's passages.
fn reference_chunks(sidecar: &SidecarReviewContext) -> Vec<(&str, &str)> {
    sidecar
        .passages
        .iter()
        .map(|passage| (passage.source_id.as_str(), passage.text.as_str()))
        .collect()
}

/// The resolved arguments for [`run_synthesize_promote`].
pub(crate) struct SynthesizePromoteArgs {
    /// Path to the pre-review candidate artifact to promote.
    pub(crate) candidate: PathBuf,
    /// Identity of the human reviewer approving this candidate for commit.
    pub(crate) reviewed_by: String,
    /// Directory holding the committed law artifacts and their index.
    pub(crate) laws_dir: PathBuf,
}

/// Handle `synthesize promote`: stamp a human-reviewed candidate, re-verify
/// it, and promote it into `laws_dir` (ADR-0016 Phase 2e).
///
/// **Concurrency:** each individual write (the law artifact, the index) is
/// atomic ([`write_atomic`]), but the index read-modify-write as a whole is
/// not locked across the two. Two `synthesize promote` invocations racing
/// against the same `laws_dir` can both read the same `index.json`, and
/// whichever writes its updated index last wins — silently dropping the
/// other's new entry (its law *file* still lands via `write_atomic`, so it
/// becomes exactly the unindexed-orphan case [`refuse_if_already_promoted`]
/// already recovers from on a later run, just via a race instead of a crash).
/// This is acceptable for a sequential, reviewer-driven CLI command — nobody
/// is expected to run two promotions concurrently against the same laws
/// directory — so no locking is implemented; if that assumption ever stops
/// holding, add a `source.lock`-style advisory lock around the read-index →
/// write-index span.
///
/// Order of operations, each a fail-fast gate before the next:
/// 1. reject a blank `--reviewed-by` (the review gate itself), before any
///    filesystem work;
/// 2. read the candidate, then validate its `law` id ([`validate_law_slug`])
///    before any path is built from it;
/// 3. read the candidate's review-context sidecar;
/// 4. stamp `synthesis.reviewed_by` — every other `synthesis` field,
///    including `candidate_sha256`, is left untouched, since it records how
///    the candidate was drafted, not how it was reviewed;
/// 5. re-run the anti-leak gate against the recorded grounding passages
///    ([`candidate_leak_texts`]), since a human edit could reintroduce
///    verbatim wording — and refuse to promote at all if the sidecar records
///    no *usable* passages (empty, or every passage under the
///    [`MIN_SHINGLE_WORDS`] shingle width), rather than silently treating
///    that as nothing to check;
/// 6. validate the stamped result via `monomyth_frameworks::load_law`, the
///    framework validator;
/// 7. bootstrap `laws_dir` (a brand-new directory has no index yet) and read
///    its index, treating a missing `index.json` as empty;
/// 8. refuse if the law is already registered in the index (idempotency) —
///    an unindexed orphan law file is a warning, not a hard block;
/// 9. atomically write the artifact into `laws_dir` and register it in the
///    index ([`write_atomic`]).
///
/// # Errors
///
/// Fails on a blank `--reviewed-by`, if the candidate declares an unsafe
/// `law` id, if the candidate or its sidecar cannot be read or parsed, if the
/// sidecar records no grounding passages, if
/// [`monomyth_synthesis::verify_no_verbatim`] finds a verbatim overlap, if
/// [`monomyth_frameworks::load_law`] rejects the stamped result, if the law
/// is already registered in the index, if the laws directory cannot be
/// created, if an existing laws index cannot be read, or if any write fails.
pub(crate) fn run_synthesize_promote(args: SynthesizePromoteArgs) -> Result<()> {
    let SynthesizePromoteArgs {
        candidate,
        reviewed_by,
        laws_dir,
    } = args;

    require_reviewer_identity(&reviewed_by)?;

    let mut artifact = read_candidate_artifact(&candidate)?;
    validate_law_slug(&artifact.law).with_context(|| {
        format!(
            "candidate {} declares an unsafe law id",
            candidate.display()
        )
    })?;
    let sidecar_path = sidecar_path_for(&candidate)?;
    let sidecar = read_sidecar_context(&sidecar_path)?;

    // Persist the trimmed identity: require_reviewer_identity already trims to check
    // for emptiness, and the untrimmed raw value should never reach the artifact.
    reviewed_by
        .trim()
        .clone_into(&mut artifact.synthesis.reviewed_by);

    let reference_chunks = reference_chunks(&sidecar);
    let has_usable_passage = reference_chunks
        .iter()
        .any(|(_, text)| text.split_whitespace().count() >= MIN_SHINGLE_WORDS);
    if !has_usable_passage {
        bail!(
            "cannot re-verify law {:?}: sidecar {} records no usable grounding passages (empty, \
             or every passage is under the {MIN_SHINGLE_WORDS}-word anti-leak shingle width); an \
             empty or tampered passages array must not silently disable the anti-leak gate",
            artifact.law,
            sidecar_path.display()
        );
    }

    verify_no_verbatim(&candidate_leak_texts(&artifact), &reference_chunks).with_context(|| {
        format!(
            "anti-leak re-check failed for law {:?}; a human edit must not reintroduce verbatim \
             wording from the recorded sources",
            artifact.law
        )
    })?;

    let stamped_json =
        serde_json::to_string_pretty(&artifact).context("serializing the stamped law artifact")?;
    load_law(&stamped_json).with_context(|| {
        format!(
            "promoted law {:?} failed monomyth_frameworks::load_law validation",
            artifact.law
        )
    })?;

    // Bootstrap the directory before reading its index: a brand-new `laws_dir`
    // (nothing promoted into it yet) has no `index.json`, and read_laws_index's
    // missing-file fallback only helps once the directory itself is there to look in.
    std::fs::create_dir_all(&laws_dir)
        .with_context(|| format!("creating laws directory {}", laws_dir.display()))?;

    let index_path = laws_dir.join("index.json");
    let mut index = read_laws_index(&index_path)?;
    refuse_if_already_promoted(&laws_dir, &artifact.law, &index)?;

    let law_path = laws_dir.join(format!("{}.json", artifact.law));
    write_atomic(&law_path, &stamped_json)
        .with_context(|| format!("writing promoted law artifact {}", law_path.display()))?;

    index.laws.push(LawIndexEntry {
        law: artifact.law.clone(),
        count: artifact.count,
        namespace: artifact.namespace.clone(),
        tier: artifact.tier.clone(),
    });
    let index_json =
        serde_json::to_string_pretty(&index).context("serializing the updated laws index")?;
    write_atomic(&index_path, &index_json)
        .with_context(|| format!("writing updated laws index {}", index_path.display()))?;

    println!("promoted law {:?} -> {}", artifact.law, law_path.display());
    Ok(())
}

#[cfg(test)]
mod tests;
