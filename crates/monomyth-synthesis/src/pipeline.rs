//! The build-time law-drafting pipeline (ADR-0016 Phase 2b/2c).
//!
//! [`draft_law`] composes the full retrieve -> distill -> gate -> stamp
//! sequence: retrieve reference-namespace passages as priors, ask the LLM to
//! distill an abstract structural taxonomy from them, run the machine-checked
//! anti-leak gate, and stamp a pre-review [`LawArtifact`]. The artifact's
//! `synthesis.reviewed_by` is always left empty here — [`load_law`] refuses to
//! load a law with an empty reviewer, so a drafted candidate is structurally
//! incapable of shipping until a human reviews it and fills that field in.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use monomyth_frameworks::{LawArtifact, LawItem, LawSynthesis};
use monomyth_knowledge::{Knowledge, KnowledgeQuery, Passage};
use monomyth_llm::{Generated, Llm, Usage};
use sha2::{Digest, Sha256};

use crate::antileak::verify_no_verbatim;
use crate::candidate::CandidateLaw;
use crate::error::SynthesisError;

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
    /// Maximum number of reference passages to retrieve as grounding.
    pub top_k: u32,
    /// A provenance label for the model that produced the candidate (e.g.
    /// `"anthropic/claude-sonnet-4-20250514"`), recorded in
    /// [`LawSynthesis::model`].
    pub model: String,
    /// An ISO 8601 date supplied by the caller and recorded verbatim in
    /// [`LawSynthesis::generated`]. The crate reads no wall clock, so
    /// generation stays deterministic and reproducible from its inputs.
    pub generated: String,
}

/// The result of a successful [`draft_law`] call.
#[derive(Clone, Debug)]
pub struct DraftedLaw {
    /// The raw model output, before pipeline stamping.
    pub candidate: CandidateLaw,
    /// The pre-review artifact. `synthesis.reviewed_by` is always empty, so
    /// [`monomyth_frameworks::load_law`] refuses to load it until a human
    /// reviews and stamps a reviewer.
    pub artifact: LawArtifact,
    /// The reference passages retrieved as grounding for the distillation.
    pub passages: Vec<Passage>,
    /// Token usage for the distillation call, when the backend reported one.
    pub usage: Option<Usage>,
    /// The sha256 hex digest of the candidate's canonical JSON, for audit
    /// trail (also recorded in [`LawSynthesis::candidate_sha256`]).
    pub candidate_sha256: String,
}

/// Draft one candidate law artifact from reference-namespace priors.
///
/// Steps: retrieve reference passages for `request.query`; refuse if none are
/// found ([`SynthesisError::NoReferenceGrounding`]) or if any retrieved
/// passage reports itself surfaceable
/// ([`SynthesisError::SurfaceablePassageInReferenceQuery`]); ask `llm` to
/// distill an abstract taxonomy from the passages; run the anti-leak gate
/// over the candidate's text against the retrieved passages
/// ([`SynthesisError::VerbatimOverlap`] on a hit — no artifact is constructed
/// in that case); then stamp a pre-review [`LawArtifact`].
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
    let passages = knowledge
        .retrieve(KnowledgeQuery::reference(&request.query, request.top_k))
        .await?;

    if passages.is_empty() {
        return Err(SynthesisError::NoReferenceGrounding {
            query: request.query.clone(),
        });
    }
    if let Some(surfaceable) = passages.iter().find(|passage| passage.is_surfaceable()) {
        return Err(SynthesisError::SurfaceablePassageInReferenceQuery {
            source_id: surfaceable.source_id.clone(),
        });
    }

    let prompt = build_distillation_prompt(&passages);
    let Generated {
        value: candidate,
        usage,
    } = llm
        .generate::<CandidateLaw>(&prompt, CANDIDATE_SCHEMA_NAME)
        .await?;

    verify_candidate_against_passages(&candidate, &passages)?;

    let candidate_sha256 = candidate_sha256(&candidate);
    let artifact = stamp_artifact(request, &candidate, &passages, &candidate_sha256);

    Ok(DraftedLaw {
        candidate,
        artifact,
        passages,
        usage,
        candidate_sha256,
    })
}

/// Build the distillation prompt.
///
/// The prompt is the anti-leak gate's first line of defense (the machine-checked
/// gate in [`crate::antileak`] is the second, structural backstop): it instructs
/// the model to output only an abstract structural taxonomy — general
/// stage/role/phase names and one-sentence structural descriptions — and
/// explicitly forbids quoting, close paraphrase, or copying wording/proper
/// nouns from the supplied passages. The passages are included as background
/// context the model reasons *over*, not text it is asked to reproduce.
fn build_distillation_prompt(passages: &[Passage]) -> String {
    let mut prompt = String::from(
        "You are distilling an ABSTRACT STRUCTURAL TAXONOMY from the reference passages below. \
         These passages are background priors only, informing your understanding of a narrative \
         or character structure that recurs across the material.\n\n\
         Output a taxonomy: a short title, and an ordered list of items, each with a general \
         name (a stage, role, or phase label) and a one-sentence ABSTRACT structural description.\n\n\
         Strict rules:\n\
         - Do NOT quote any passage.\n\
         - Do NOT closely paraphrase any passage's wording or sentence structure.\n\
         - Do NOT copy proper nouns, character names, or specific story details from the \
           passages into your output.\n\
         - Describe the STRUCTURE only (the recurring pattern), never a specific instance of it.\n\n\
         Reference passages (priors only, never to be reproduced):\n",
    );
    for (index, passage) in passages.iter().enumerate() {
        let position = index + 1;
        // `write!` to a `String` never fails, so the result is discarded.
        let _ = writeln!(prompt, "\n[{position}] {}", passage.text);
    }
    prompt
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
