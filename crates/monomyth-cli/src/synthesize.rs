//! The `synthesize law` subcommand: draft a pre-review candidate law artifact
//! from reference-namespace priors (ADR-0016 Phase 2d).
//!
//! `monomyth-synthesis::draft_law` produces a [`monomyth_synthesis::DraftedLaw`]
//! whose `artifact.synthesis.reviewed_by` is always empty — structurally
//! incapable of loading via [`monomyth_frameworks::load_law`] until a human
//! reviews it. This module's whole job is to make that review boundary
//! impossible to miss: candidates are written to a gitignored `synthesis/`
//! directory, never to `artifacts/`, and every run ends with a REVIEW-REQUIRED
//! banner on stderr spelling out the promotion steps.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use monomyth_config::{ConfigResolver, ModelRole, RuntimeOverrides};
use monomyth_knowledge::Knowledge;
use monomyth_llm::{BackendOptions, Llm};
use monomyth_synthesis::{
    CoverageFramework, DraftRequest, DraftedLaw, JudgeVerdict, LoopConfig, PreScore,
    coverage_sub_queries, draft_law,
};
use serde::Serialize;
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
         sources.\n\
         2. Edit wording as needed — the model's authority was the abstract idea only.\n\
         3. Set synthesis.reviewed_by to your identity.\n\
         4. Move the file into artifacts/laws/ and add an entry to artifacts/laws/index.json.\n\
         5. Run the framework validator.\n",
        artifact_path.display(),
        context_path.display(),
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
/// Fails if `out_dir` cannot be created, either file cannot be serialized, or
/// either file cannot be written.
fn write_candidate(
    drafted: &DraftedLaw,
    law_id: &str,
    query: &str,
    out_dir: &Path,
) -> Result<PathBuf> {
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use monomyth_frameworks::{LawArtifact, LawError, LawItem, LawSynthesis, load_law};
    use monomyth_knowledge::{Namespace, Passage};
    use monomyth_synthesis::CandidateLaw;

    use super::{DraftedLaw, rejects_forbidden_out_dir, review_required_banner, write_candidate};

    #[test]
    fn should_refuse_a_path_directly_under_artifacts() {
        let reason = rejects_forbidden_out_dir(Path::new("./artifacts/laws"));
        assert!(reason.is_some(), "./artifacts/laws must be refused");
        assert!(reason.unwrap().contains("synthesis/candidates"));
    }

    #[test]
    fn should_refuse_bare_artifacts_directory() {
        assert!(rejects_forbidden_out_dir(Path::new("artifacts")).is_some());
    }

    #[test]
    fn should_refuse_artifacts_nested_deep_in_the_path() {
        assert!(rejects_forbidden_out_dir(Path::new("foo/artifacts/candidates")).is_some());
    }

    #[test]
    fn should_accept_the_default_candidates_directory() {
        assert!(rejects_forbidden_out_dir(Path::new("./synthesis/candidates")).is_none());
    }

    #[test]
    fn should_accept_an_unrelated_relative_directory() {
        assert!(rejects_forbidden_out_dir(Path::new("./out")).is_none());
    }

    #[test]
    fn should_accept_an_unrelated_absolute_directory() {
        assert!(rejects_forbidden_out_dir(Path::new("/tmp/x")).is_none());
    }

    /// Build a hand-made [`DraftedLaw`] for offline testing — no LLM, no
    /// retrieval. `synthesis.reviewed_by` is left empty, matching what
    /// `draft_law` always produces.
    fn sample_drafted_law() -> DraftedLaw {
        let items = vec![
            LawItem {
                id: 1,
                name: "Setup".to_owned(),
                description: "The ordinary world before the call.".to_owned(),
                fields: BTreeMap::new(),
            },
            LawItem {
                id: 2,
                name: "Confrontation".to_owned(),
                description: "The central structural conflict.".to_owned(),
                fields: BTreeMap::new(),
            },
        ];
        let artifact = LawArtifact {
            law: "sample_law".to_owned(),
            title: "Sample Law".to_owned(),
            license: "idea/taxonomy only; synthesized structural law".to_owned(),
            namespace: "ship".to_owned(),
            tier: "system".to_owned(),
            domain: "myth".to_owned(),
            tier_note: "test fixture".to_owned(),
            synthesis: LawSynthesis {
                reference_source_ids: vec!["some_reference_source".to_owned()],
                model: "test/stub-model".to_owned(),
                generated: "2026-07-12".to_owned(),
                reviewed_by: String::new(),
                candidate_sha256: Some("deadbeef".to_owned()),
            },
            count: items.len(),
            items,
        };
        let passage = Passage {
            text: "some reference passage text".to_owned(),
            source_id: "some_reference_source".to_owned(),
            score: 0.9,
            namespace: Namespace::Reference,
            license: "in-copyright".to_owned(),
            url: None,
            checksum: None,
            retrieved: None,
        };
        DraftedLaw {
            candidate: CandidateLaw {
                title: "Sample Law".to_owned(),
                items: vec![],
            },
            artifact,
            passages: vec![passage],
            usage: None,
            candidate_sha256: "deadbeef".to_owned(),
            final_score: 90.0,
            iterations: 1,
            verdict: None,
            pre_score: None,
        }
    }

    #[test]
    fn should_write_both_candidate_files_and_round_trip_the_artifact() {
        let temp_dir = tempfile::tempdir().expect("tempdir creates");
        let drafted = sample_drafted_law();

        let artifact_path =
            write_candidate(&drafted, "sample_law", "a sample query", temp_dir.path())
                .expect("writing the candidate succeeds");
        let context_path = temp_dir.path().join("sample_law.context.json");

        assert!(artifact_path.exists(), "artifact file must exist");
        assert!(context_path.exists(), "context file must exist");
        assert_eq!(artifact_path, temp_dir.path().join("sample_law.json"));

        let context_json = std::fs::read_to_string(&context_path).expect("context file reads back");
        assert!(
            context_json.contains("a sample query"),
            "context file must record the grounding query for the reviewer"
        );
        assert!(
            context_json.contains("\"final_score\": 90.0"),
            "context file must record the judge's final score for the reviewer"
        );
        assert!(
            context_json.contains("\"iterations\": 1"),
            "context file must record the judge iteration count for the reviewer"
        );

        let artifact_json =
            std::fs::read_to_string(&artifact_path).expect("artifact file reads back");
        let round_tripped: LawArtifact =
            serde_json::from_str(&artifact_json).expect("artifact JSON parses");
        assert_eq!(round_tripped.law, "sample_law");
        assert_eq!(round_tripped.count, 2);
        assert_eq!(round_tripped.items.len(), 2);

        // A written candidate must be structurally incapable of shipping: its ~keep
        // empty `reviewed_by` makes `load_law` refuse it outright. ~keep
        let load_result = load_law(&artifact_json);
        assert!(
            matches!(load_result, Err(LawError::MissingReviewer { .. })),
            "an unreviewed candidate must fail load_law with MissingReviewer, got {load_result:?}"
        );
    }

    #[test]
    fn default_config_synthesis_knobs_match_loop_config_default() {
        use monomyth_config::ConfigResolver;
        use monomyth_synthesis::LoopConfig;

        let config = ConfigResolver::defaults()
            .resolve()
            .expect("defaults validate");
        let loop_default = LoopConfig::default();
        assert_eq!(
            *config.synthesis.max_iterations.get(),
            loop_default.max_iterations
        );
        assert_eq!(
            *config.synthesis.per_query_top_k.get(),
            loop_default.per_query_top_k
        );
        assert_eq!(
            *config.synthesis.max_grounding.get(),
            loop_default.max_grounding
        );
    }

    #[test]
    fn banner_text_should_name_the_review_boundary() {
        let banner = review_required_banner(
            "sample_law",
            Path::new("./synthesis/candidates/sample_law.json"),
            Path::new("./synthesis/candidates/sample_law.context.json"),
            82.0,
            2,
        );
        assert!(
            banner.contains("reviewed_by"),
            "banner must mention reviewed_by"
        );
        assert!(
            banner.contains("artifacts/laws"),
            "banner must mention artifacts/laws"
        );
        assert!(banner.contains("REVIEW REQUIRED"));
        assert!(
            banner.contains("Judge score: 82/100 after 2 iteration(s)"),
            "banner must surface the judge score and iteration count"
        );
    }
}
