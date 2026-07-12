//! The build-time law-drafting pipeline (ADR-0016 Phase 2b/2c).
//!
//! [`draft_law`] composes the full retrieve -> distill -> judge -> refine ->
//! gate -> stamp sequence: gather multi-query reference-namespace passages as
//! priors, ask the LLM to distill an abstract structural taxonomy from them,
//! score the candidate with an LLM judge against weighted criteria, and — while
//! the score is below a rising passing bar and iterations remain — retrieve
//! targeted grounding for the phases the judge names missing and re-draft. The
//! best-scoring candidate then runs the machine-checked anti-leak gate and is
//! stamped into a pre-review [`LawArtifact`]. The artifact's
//! `synthesis.reviewed_by` is always left empty here — [`load_law`] refuses to
//! load a law with an empty reviewer, so a drafted candidate is structurally
//! incapable of shipping until a human reviews it and fills that field in.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use monomyth_frameworks::{LawArtifact, LawItem, LawSynthesis};
use monomyth_knowledge::{Knowledge, Passage};
use monomyth_llm::{Generated, Llm, Usage};
use sha2::{Digest, Sha256};

use crate::antileak::verify_no_verbatim;
use crate::candidate::CandidateLaw;
use crate::error::SynthesisError;
use crate::judge::{DEFAULT_CRITERIA, JudgeVerdict, judge_candidate, weighted_score};
use crate::retrieval::{DEFAULT_PER_QUERY_TOP_K, MAX_GROUNDING_PASSAGES, gather_grounding};

/// The `schemars`/`Llm::generate` schema name for [`CandidateLaw`].
const CANDIDATE_SCHEMA_NAME: &str = "CandidateLaw";

/// The `namespace` stamped onto every drafted [`LawArtifact`]: a synthesized
/// law is always ship-safe by construction (idea/taxonomy only).
const ARTIFACT_NAMESPACE: &str = "ship";

/// The `tier` stamped onto every drafted [`LawArtifact`]: the model's output is
/// an uncopyrightable structural idea, never reproduced source prose.
const ARTIFACT_TIER: &str = "system";

/// The `license` note stamped onto every drafted [`LawArtifact`].
const ARTIFACT_LICENSE: &str =
    "idea/taxonomy only; synthesized structural law, no source prose reproduced";

/// The `tier_note` stamped onto every drafted [`LawArtifact`], explaining the
/// ship-safety rationale in human-readable form for a reviewer.
const ARTIFACT_TIER_NOTE: &str = "Synthesized from reference-namespace priors via LLM distillation and passed the \
     machine-checked anti-leak gate (no verbatim 8-word overlap with any source passage). The \
     model's authority was limited to the abstract taxonomy; wording and item ids were stamped \
     by the pipeline, not the model. Requires human review (synthesis.reviewed_by) before it may \
     load.";

/// Default maximum number of judge/refine iterations in the feedback loop.
///
/// Three iterations (one initial draft plus up to two refinements) is enough
/// headroom to recover from an under-collected first pass without letting a
/// stuck loop burn unbounded LLM calls.
const DEFAULT_MAX_ITERATIONS: u32 = 3;

/// Default weighted score a candidate must reach on the first judged iteration
/// to be accepted outright.
const DEFAULT_INITIAL_PASSING_SCORE: f64 = 70.0;

/// Default amount the passing bar rises per iteration.
///
/// The bar rises rather than staying flat so that a candidate which has
/// already consumed a refinement round is held to a somewhat higher standard
/// than one that passed on the first try — the loop is meant to converge, not
/// to let a mediocre candidate squeak through on a technicality after
/// spending its retries.
const DEFAULT_SCORE_INCREMENT: f64 = 5.0;

/// Configuration for the judge feedback loop in [`draft_law`].
#[derive(Clone, Debug)]
pub struct LoopConfig {
    /// Maximum number of judge/refine iterations. The initial draft always
    /// happens; this bounds how many times the loop may judge-and-refine
    /// after it.
    pub max_iterations: u32,
    /// The weighted score (see [`weighted_score`]) a candidate must reach on
    /// the first iteration to be accepted.
    pub initial_passing_score: f64,
    /// The amount added to the passing bar on each subsequent iteration (see
    /// [`DEFAULT_SCORE_INCREMENT`] for why the bar rises rather than staying
    /// flat).
    pub score_increment: f64,
    /// `top_k` passed to each individual query in [`gather_grounding`].
    pub per_query_top_k: u32,
    /// Maximum total grounding passages retained by [`gather_grounding`]
    /// after dedup, across all queries.
    pub max_grounding: usize,
}

impl Default for LoopConfig {
    fn default() -> Self {
        Self {
            max_iterations: DEFAULT_MAX_ITERATIONS,
            initial_passing_score: DEFAULT_INITIAL_PASSING_SCORE,
            score_increment: DEFAULT_SCORE_INCREMENT,
            per_query_top_k: DEFAULT_PER_QUERY_TOP_K,
            max_grounding: MAX_GROUNDING_PASSAGES,
        }
    }
}

/// A request to draft one candidate law.
#[derive(Clone, Debug)]
pub struct DraftRequest {
    /// The machine-readable law identifier to stamp onto the artifact (e.g.
    /// `"three_act"`). Never seen by the model.
    pub law_id: String,
    /// The corpus domain this law belongs to (e.g. `"myth"`, `"folklore"`).
    pub domain: String,
    /// The retrieval query issued against the reference collection.
    pub query: String,
    /// Additional coverage queries unioned with `query` for the initial
    /// retrieval (see [`gather_grounding`]). Empty is fine — the loop still
    /// works from `query` alone, and gains targeted queries as the judge
    /// names missing phases.
    pub sub_queries: Vec<String>,
    /// A provenance label for the model that produced the candidate (e.g.
    /// `"anthropic/claude-sonnet-5"`), recorded in [`LawSynthesis::model`].
    /// The caller supplies it; the CLI resolves it from its configured default
    /// rather than hardcoding a model here.
    pub model: String,
    /// An ISO 8601 date supplied by the caller and recorded verbatim in
    /// [`LawSynthesis::generated`]. The crate reads no wall clock, so
    /// generation stays deterministic and reproducible from its inputs.
    pub generated: String,
    /// Configuration for the judge feedback loop.
    pub loop_config: LoopConfig,
}

/// The result of a successful [`draft_law`] call.
#[derive(Clone, Debug)]
pub struct DraftedLaw {
    /// The raw model output, before pipeline stamping. This is the
    /// best-scoring candidate across all iterations, not necessarily the
    /// last one drafted.
    pub candidate: CandidateLaw,
    /// The pre-review artifact. `synthesis.reviewed_by` is always empty, so
    /// [`monomyth_frameworks::load_law`] refuses to load it until a human
    /// reviews and stamps a reviewer.
    pub artifact: LawArtifact,
    /// The reference passages retrieved as grounding for the distillation,
    /// including any targeted passages folded in by mid-loop refinement.
    pub passages: Vec<Passage>,
    /// Token usage for the distillation call, when the backend reported one.
    ///
    /// Reports only the usage of the call that produced [`Self::candidate`]
    /// (not the judge calls or any discarded refinement drafts), so it
    /// answers "what did the accepted candidate cost to draft".
    pub usage: Option<Usage>,
    /// The sha256 hex digest of the candidate's canonical JSON, for audit
    /// trail (also recorded in [`LawSynthesis::candidate_sha256`]).
    pub candidate_sha256: String,
    /// The best-scoring candidate's weighted judge score (see
    /// [`weighted_score`]).
    pub final_score: f64,
    /// How many judge/refine iterations ran before the loop stopped
    /// (accepted, or exhausted [`LoopConfig::max_iterations`]).
    pub iterations: u32,
    /// The best-scoring candidate's judge verdict, when at least one judge
    /// call succeeded.
    pub verdict: Option<JudgeVerdict>,
}

/// One iteration's candidate together with the state needed to track the
/// best-scoring candidate across the loop.
struct IterationResult {
    candidate: CandidateLaw,
    usage: Option<Usage>,
    score: f64,
    verdict: JudgeVerdict,
}

/// Draft one candidate law artifact from reference-namespace priors, via an
/// LLM-judge feedback loop.
///
/// Steps: gather multi-query grounding for `request.query` plus
/// `request.sub_queries`; refuse if none are found
/// ([`SynthesisError::NoReferenceGrounding`]); ask the LLM to distill an
/// initial candidate; then, for up to `request.loop_config.max_iterations`,
/// judge the candidate against [`DEFAULT_CRITERIA`], accept it once its
/// weighted score clears a rising bar, or else retrieve targeted grounding for
/// the judge's named missing phases and ask the LLM to refine the candidate.
/// The best-scoring candidate across all iterations then runs the anti-leak
/// gate over the full accumulated grounding
/// ([`SynthesisError::VerbatimOverlap`] on a hit — no artifact is constructed
/// in that case) and is stamped into a pre-review [`LawArtifact`].
///
/// # Errors
///
/// See [`SynthesisError`]'s variants for the specific failure modes above,
/// plus [`SynthesisError::Retrieval`] and [`SynthesisError::Generation`] for
/// transport-level failures.
pub async fn draft_law(
    knowledge: &Knowledge,
    llm: &Llm,
    request: &DraftRequest,
) -> Result<DraftedLaw, SynthesisError> {
    let config = &request.loop_config;

    let mut queries = vec![request.query.clone()];
    queries.extend(request.sub_queries.iter().cloned());
    let mut grounding = gather_grounding(
        knowledge,
        &queries,
        config.per_query_top_k,
        config.max_grounding,
    )
    .await?;

    if grounding.is_empty() {
        return Err(SynthesisError::NoReferenceGrounding {
            query: request.query.clone(),
        });
    }

    let initial_prompt = build_distillation_prompt(&grounding);
    let Generated {
        value: mut candidate,
        usage: mut current_usage,
    } = llm
        .generate::<CandidateLaw>(&initial_prompt, CANDIDATE_SCHEMA_NAME)
        .await?;

    let mut best: Option<IterationResult> = None;
    let mut iterations = 0;

    for iteration_index in 0..config.max_iterations {
        iterations += 1;
        let bar =
            config.initial_passing_score + f64::from(iteration_index) * config.score_increment;

        let verdict = judge_candidate(llm, &candidate, &grounding, DEFAULT_CRITERIA).await?;
        let score = weighted_score(&verdict, DEFAULT_CRITERIA);

        let is_new_best = best
            .as_ref()
            .is_none_or(|current_best| score > current_best.score);
        if is_new_best {
            best = Some(IterationResult {
                candidate: candidate.clone(),
                usage: current_usage.clone(),
                score,
                verdict: verdict.clone(),
            });
        }

        if score >= bar {
            break;
        }

        let has_more_iterations = iteration_index + 1 < config.max_iterations;
        if !has_more_iterations {
            break;
        }

        if !verdict.missing_phases.is_empty() {
            let targeted = gather_grounding(
                knowledge,
                &verdict.missing_phases,
                config.per_query_top_k,
                config.max_grounding,
            )
            .await?;
            grounding = merge_grounding(grounding, targeted, config.max_grounding);
        }

        let refine_prompt = build_refine_prompt(&candidate, &verdict, &grounding);
        let Generated {
            value: refined,
            usage: refined_usage,
        } = llm
            .generate::<CandidateLaw>(&refine_prompt, CANDIDATE_SCHEMA_NAME)
            .await?;
        candidate = refined;
        current_usage = refined_usage;
    }

    let IterationResult {
        candidate: best_candidate,
        usage: best_usage,
        score: final_score,
        verdict: best_verdict,
    } = best.unwrap_or(IterationResult {
        candidate,
        usage: current_usage,
        score: 0.0,
        verdict: JudgeVerdict {
            criteria: Vec::new(),
            missing_phases: Vec::new(),
            instructions: Vec::new(),
        },
    });

    verify_candidate_against_passages(&best_candidate, &grounding)?;

    let candidate_sha256 = candidate_sha256(&best_candidate);
    let artifact = stamp_artifact(request, &best_candidate, &grounding, &candidate_sha256);

    Ok(DraftedLaw {
        candidate: best_candidate,
        artifact,
        passages: grounding,
        usage: best_usage,
        candidate_sha256,
        final_score,
        iterations,
        verdict: Some(best_verdict),
    })
}

/// Merge `additional` passages into `existing`, deduplicating by
/// `(source_id, normalized text)` (keeping the higher score on a collision),
/// then re-sort by score descending and truncate to `cap`.
///
/// Reuses the same normalization rule as [`crate::retrieval::gather_grounding`]
/// so a passage already present from the initial retrieval is recognized as a
/// duplicate when it resurfaces from a targeted mid-loop query.
fn merge_grounding(existing: Vec<Passage>, additional: Vec<Passage>, cap: usize) -> Vec<Passage> {
    let mut deduped: std::collections::BTreeMap<(String, String), Passage> =
        std::collections::BTreeMap::new();

    for passage in existing.into_iter().chain(additional) {
        let key = (
            passage.source_id.clone(),
            normalize_for_merge(&passage.text),
        );
        match deduped.get(&key) {
            Some(current) if current.score >= passage.score => {}
            _ => {
                deduped.insert(key, passage);
            }
        }
    }

    let mut merged: Vec<Passage> = deduped.into_values().collect();
    merged.sort_by(|left, right| {
        right
            .score
            .partial_cmp(&left.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    merged.truncate(cap);
    merged
}

/// Normalize `text` for dedup comparison, matching
/// [`crate::retrieval::gather_grounding`]'s normalization rule so a passage
/// merged mid-loop is recognized as a duplicate of one already present.
fn normalize_for_merge(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Build the initial distillation prompt.
///
/// The prompt is the anti-leak gate's first line of defense (the machine-checked
/// gate in [`crate::antileak`] is the second, structural backstop): it instructs
/// the model to output only an abstract structural taxonomy — general
/// stage/role/phase names and one-sentence structural descriptions — and
/// explicitly forbids quoting, close paraphrase, or copying wording/proper
/// nouns from the supplied passages. It also demands exhaustiveness: the
/// dominant failure mode observed in practice is under-collection (a source
/// supporting a dozen distinct phases flattened into three or four coarse
/// buckets), so the prompt explicitly asks for macro-tier granularity across
/// the whole arc, including the return/restoration, rather than stopping once
/// a plausible-looking handful of items has been produced.
fn build_distillation_prompt(passages: &[Passage]) -> String {
    let mut prompt = String::from(
        "You are distilling an ABSTRACT STRUCTURAL TAXONOMY from the reference passages below. \
         These passages are background priors only, informing your understanding of a narrative \
         or character structure that recurs across the material.\n\n\
         Output a taxonomy: a short title, and an ordered list of items, each with a general \
         name (a stage, role, or phase label) and a one-sentence ABSTRACT structural description.\n\n\
         Exhaustiveness is critical: enumerate EVERY distinct structural phase implied by the \
         grounding, in narrative order, across the WHOLE arc — from the initial departure, \
         through the trials and crux, to the return and restoration. Do not merge two distinct \
         phases into one coarse bucket just because they are adjacent; if the grounding supports \
         separating a beat into two phases, list them as two items. When the material supports \
         it, target macro-tier granularity: on the order of a dozen or more distinct phases, not \
         a handful.\n\n\
         Strict rules:\n\
         - Do NOT quote any passage.\n\
         - Do NOT closely paraphrase any passage's wording or sentence structure.\n\
         - Do NOT copy proper nouns, character names, or specific story details from the \
           passages into your output.\n\
         - Describe the STRUCTURE only (the recurring pattern), never a specific instance of it.\n\n\
         Reference passages (priors only, never to be reproduced):\n",
    );
    append_passages(&mut prompt, passages);
    prompt
}

/// Build the refine prompt for a mid-loop iteration.
///
/// Shows the model its own current candidate, the judge's `missing_phases`
/// and `instructions`, and the (possibly augmented) grounding, and instructs
/// it to EXPAND or refine the candidate — adding the missing phases and
/// addressing the instructions — while returning a complete, self-contained
/// [`CandidateLaw`] (not a diff) and keeping the same anti-leak-by-construction
/// rules as the initial distillation prompt.
fn build_refine_prompt(
    candidate: &CandidateLaw,
    verdict: &JudgeVerdict,
    grounding: &[Passage],
) -> String {
    let mut prompt = String::from(
        "You are REFINING an abstract structural taxonomy you previously drafted, based on an \
         evaluator's feedback. Return a COMPLETE, self-contained taxonomy (title and the full \
         ordered list of items) — not a diff or a list of additions only.\n\n\
         Your current candidate:\n",
    );
    let _ = writeln!(prompt, "Title: {}", candidate.title);
    for (index, item) in candidate.items.iter().enumerate() {
        let position = index + 1;
        let _ = writeln!(prompt, "{position}. {}: {}", item.name, item.description);
    }

    prompt.push_str("\nThe evaluator found these structural phases missing or underdeveloped:\n");
    for phase in &verdict.missing_phases {
        let _ = writeln!(prompt, "- {phase}");
    }

    prompt.push_str("\nThe evaluator's improvement instructions:\n");
    for instruction in &verdict.instructions {
        let _ = writeln!(prompt, "- {instruction}");
    }

    prompt.push_str(
        "\nExpand and refine the candidate to add the missing phases (in correct narrative order \
         relative to the existing items) and address the instructions, while keeping every \
         existing item that the evaluator did not flag as a problem.\n\n\
         Strict rules (unchanged from the original draft):\n\
         - Do NOT quote any passage.\n\
         - Do NOT closely paraphrase any passage's wording or sentence structure.\n\
         - Do NOT copy proper nouns, character names, or specific story details from the \
           passages into your output.\n\
         - Describe the STRUCTURE only (the recurring pattern), never a specific instance of it.\n\n\
         Reference passages (priors only, never to be reproduced):\n",
    );
    append_passages(&mut prompt, grounding);
    prompt
}

/// Append numbered `passages` to `prompt`, shared by the distillation and
/// refine prompt builders.
fn append_passages(prompt: &mut String, passages: &[Passage]) {
    for (index, passage) in passages.iter().enumerate() {
        let position = index + 1;
        // `write!` to a `String` never fails, so the result is discarded.
        let _ = writeln!(prompt, "\n[{position}] {}", passage.text);
    }
}

/// Run the anti-leak gate over every text field of `candidate` against every
/// retrieved `passages`' text.
fn verify_candidate_against_passages(
    candidate: &CandidateLaw,
    passages: &[Passage],
) -> Result<(), SynthesisError> {
    let mut candidate_texts: Vec<&str> = vec![candidate.title.as_str()];
    for item in &candidate.items {
        candidate_texts.push(item.name.as_str());
        candidate_texts.push(item.description.as_str());
    }

    let reference_chunks: Vec<(&str, &str)> = passages
        .iter()
        .map(|passage| (passage.source_id.as_str(), passage.text.as_str()))
        .collect();

    verify_no_verbatim(&candidate_texts, &reference_chunks)
}

/// The sha256 hex digest of the canonical (`serde_json::to_string`) JSON
/// encoding of `candidate`, for the artifact's audit trail.
///
/// [`CandidateLaw`] is composed entirely of `String` and `Vec` fields with no
/// externally-defined `Serialize` impls, so `serde_json::to_string` cannot
/// fail on it in practice; on the unreachable error path this hashes the
/// empty string rather than propagating a spurious fallible signature for a
/// call that cannot realistically fail.
fn candidate_sha256(candidate: &CandidateLaw) -> String {
    let canonical = serde_json::to_string(candidate).unwrap_or_default();
    let digest = Sha256::digest(canonical.as_bytes());
    hex_encode(&digest)
}

/// Lowercase-hex encode `bytes`, dependency-free.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut hex = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // `write!` to a `String` never fails, so the result is discarded.
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// Stamp a pre-review [`LawArtifact`] from `request`, `candidate`, the
/// retrieved `passages`, and the precomputed `candidate_sha256`.
///
/// `synthesis.reviewed_by` is always the empty string: human review is a
/// separate, later step, and [`monomyth_frameworks::load_law`] enforces that
/// an empty reviewer cannot load.
fn stamp_artifact(
    request: &DraftRequest,
    candidate: &CandidateLaw,
    passages: &[Passage],
    candidate_sha256: &str,
) -> LawArtifact {
    let items: Vec<LawItem> = candidate
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| LawItem {
            // `index` is bounded by `candidate.items.len()`, which in practice
            // is a handful of taxonomy entries; a `u16` overflow here would
            // require an implausibly large candidate.
            id: u16::try_from(index + 1).unwrap_or(u16::MAX),
            name: item.name.clone(),
            description: item.description.clone(),
            fields: std::collections::BTreeMap::new(),
        })
        .collect();

    let reference_source_ids: Vec<String> = passages
        .iter()
        .map(|passage| passage.source_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    LawArtifact {
        law: request.law_id.clone(),
        title: candidate.title.clone(),
        license: ARTIFACT_LICENSE.to_owned(),
        namespace: ARTIFACT_NAMESPACE.to_owned(),
        tier: ARTIFACT_TIER.to_owned(),
        domain: request.domain.clone(),
        tier_note: ARTIFACT_TIER_NOTE.to_owned(),
        synthesis: LawSynthesis {
            reference_source_ids,
            model: request.model.clone(),
            generated: request.generated.clone(),
            reviewed_by: String::new(),
            candidate_sha256: Some(candidate_sha256.to_owned()),
        },
        count: items.len(),
        items,
    }
}
