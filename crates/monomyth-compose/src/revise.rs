//! The Revise feedback loop: scoring a drafted section against its grounding
//! and, if it falls short, regenerating it up to a cap.
//!
//! A section's score is the cosine similarity of its own embedding against the
//! mean of its grounding passages' embeddings — a proxy for how faithful the
//! drafted prose stayed to the reference priors it was grounded on. This is
//! never an error condition: per the synthesis judge-loop's posture, compose
//! always succeeds. If the loop exhausts [`ComposeSettings::max_revise_iterations`]
//! without reaching [`ComposeSettings::revise_threshold`], the best-scoring
//! attempt is kept and annotated with a [`SectionDraft::note`] explaining why.

use monomyth_eval::cosine_similarity;
use monomyth_knowledge::{Knowledge, KnowledgeQuery};
use monomyth_llm::Llm;

use crate::assemble::SectionDraft;
use crate::error::ComposeError;
use crate::generate::generate_long_form;
use crate::outline::OutlineSection;
use crate::prompts::{grounding_query, section_draft_prompt, section_revise_prompt};
use crate::settings::ComposeSettings;

/// The elementwise mean of `vectors`.
///
/// Returns `None` when `vectors` is empty, or when the vectors are not all the
/// same length (a mismatched set has no well-defined elementwise mean; guarded
/// defensively since callers hand in embeddings from a single embedder call
/// and should never actually hit this branch).
fn mean_vector(vectors: &[Vec<f32>]) -> Option<Vec<f32>> {
    let dimension = vectors.first()?.len();
    if vectors.iter().any(|vector| vector.len() != dimension) {
        return None;
    }

    let mut sum = vec![0.0_f32; dimension];
    for vector in vectors {
        for (slot, value) in sum.iter_mut().zip(vector.iter()) {
            *slot += value;
        }
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "vector count is a small grounding top-k, exactly representable as f32"
    )]
    let count = vectors.len() as f32;
    for slot in &mut sum {
        *slot /= count;
    }
    Some(sum)
}

/// One scored drafting attempt for a section: its text and its cosine score
/// against the grounding mean.
struct ScoredAttempt {
    text: String,
    score: f64,
}

/// Draft `section`, scoring it against its REFERENCE-path grounding and
/// revising up to `settings.max_revise_iterations` times if it falls short of
/// `settings.revise_threshold`.
///
/// # Errors
///
/// Returns [`ComposeError::Retrieval`] if grounding retrieval fails,
/// [`ComposeError::Generation`] if any underlying [`generate_long_form`] call
/// fails, or [`ComposeError::NoTurns`] if `settings.max_turns == 0`. Never
/// returns an error for non-convergence: compose always succeeds, keeping the
/// best-scoring attempt with a [`SectionDraft::note`] instead.
pub(crate) async fn draft_and_revise_section(
    llm: &Llm,
    knowledge: &Knowledge,
    section: &OutlineSection,
    prior: &str,
    settings: &ComposeSettings,
) -> Result<SectionDraft, ComposeError> {
    // Reference-path retrieval only (`KnowledgeQuery::reference`), never
    // `KnowledgeQuery::surfaceable`. Reference passages inform generation as
    // priors and are never surfaced verbatim (ADR-0016); grounding must never
    // leave the reference namespace.
    let passages = knowledge
        .retrieve(KnowledgeQuery::reference(
            grounding_query(section),
            settings.grounding_top_k,
        ))
        .await?;
    let grounding: Vec<String> = passages.into_iter().map(|passage| passage.text).collect();

    if grounding.is_empty() {
        return draft_single_unscored(llm, section, &grounding, prior, settings).await;
    }

    let grounding_embeddings = knowledge.embed_texts(grounding.clone()).await?;
    let Some(gold) = mean_vector(&grounding_embeddings) else {
        // No gold to score against (embedding failure/degenerate input) —
        // scoring is impossible, so draft once and skip the revise loop.
        return draft_single_unscored(llm, section, &grounding, prior, settings).await;
    };

    revise_loop(llm, knowledge, section, &grounding, prior, settings, &gold).await
}

/// Produce a single, unscored draft: used when there is no grounding (or no
/// gold vector) to score against.
async fn draft_single_unscored(
    llm: &Llm,
    section: &OutlineSection,
    grounding: &[String],
    prior: &str,
    settings: &ComposeSettings,
) -> Result<SectionDraft, ComposeError> {
    let instruction = section_draft_prompt(section);
    let text = generate_long_form(llm, &instruction, grounding, prior, settings.max_turns).await?;
    Ok(SectionDraft {
        node_id: section.node_id,
        stage: section.stage,
        text,
        note: None,
    })
}

/// Draft `section` against `grounding`/`gold`, revising until the score meets
/// `settings.revise_threshold` or `settings.max_revise_iterations` is
/// exhausted, keeping the best-scoring attempt either way.
async fn revise_loop(
    llm: &Llm,
    knowledge: &Knowledge,
    section: &OutlineSection,
    grounding: &[String],
    prior: &str,
    settings: &ComposeSettings,
    gold: &[f32],
) -> Result<SectionDraft, ComposeError> {
    let mut best: Option<ScoredAttempt> = None;

    for _iteration in 0..=settings.max_revise_iterations {
        let instruction = match &best {
            None => section_draft_prompt(section),
            Some(previous) => section_revise_prompt(
                section,
                &previous.text,
                previous.score,
                settings.revise_threshold,
            ),
        };

        let text =
            generate_long_form(llm, &instruction, grounding, prior, settings.max_turns).await?;
        let embedded = knowledge.embed_texts(vec![text.clone()]).await?;
        // `embed_texts` returns one vector per input; a single-element input
        // always yields exactly one output vector.
        let score = embedded
            .first()
            .and_then(|vector| cosine_similarity(vector, gold))
            // Treat a non-comparable embedding (`None`) as the worst possible
            // score rather than skipping it, so a degenerate embedding can
            // never spuriously "converge" by being excluded from comparison.
            .unwrap_or(0.0);

        let converged = score >= settings.revise_threshold;
        let is_better = best.as_ref().is_none_or(|current| score > current.score);
        if is_better {
            best = Some(ScoredAttempt { text, score });
        }

        if converged {
            let winner = best.expect("just inserted this iteration's attempt");
            return Ok(SectionDraft {
                node_id: section.node_id,
                stage: section.stage,
                text: winner.text,
                note: None,
            });
        }
    }

    let winner = best.expect("the loop runs at least once (0..=N is never empty)");
    let note = format!(
        "semantic grounding score {best_score:.3} stayed below threshold {threshold:.3} after \
         {revisions} revision(s)",
        best_score = winner.score,
        threshold = settings.revise_threshold,
        revisions = settings.max_revise_iterations,
    );
    Ok(SectionDraft {
        node_id: section.node_id,
        stage: section.stage,
        text: winner.text,
        note: Some(note),
    })
}

#[cfg(test)]
mod tests {
    use monomyth_core::NarrativeNodeId;
    use monomyth_frameworks::MonomythStage;
    use monomyth_knowledge::IngestInput;

    use super::{draft_and_revise_section, mean_vector};
    use crate::outline::OutlineSection;
    use crate::settings::ComposeSettings;
    use crate::test_support::{Continuation, FakeBackend, in_memory_reference_knowledge, llm_with};

    /// A declared `reference`-namespace source id (see `corpus/manifest.json`),
    /// used to ingest reference passages for these tests' grounding.
    const REFERENCE_SOURCE_ID: &str = "perseus";

    #[test]
    fn should_compute_elementwise_mean_of_equal_length_vectors() {
        let vectors = vec![vec![1.0_f32, 2.0], vec![3.0_f32, 4.0]];

        let mean = mean_vector(&vectors).expect("equal-length vectors have a mean");

        assert_eq!(mean, vec![2.0, 3.0]);
    }

    #[test]
    fn should_compute_elementwise_mean_of_three_vectors() {
        let vectors = vec![
            vec![1.0_f32, 0.0, 3.0],
            vec![2.0_f32, 3.0, 3.0],
            vec![3.0_f32, 6.0, 3.0],
        ];

        let mean = mean_vector(&vectors).expect("equal-length vectors have a mean");

        assert_eq!(mean, vec![2.0, 3.0, 3.0]);
    }

    #[test]
    fn should_return_none_for_empty_input() {
        let vectors: Vec<Vec<f32>> = Vec::new();

        assert_eq!(mean_vector(&vectors), None);
    }

    #[test]
    fn should_return_none_for_mismatched_lengths() {
        let vectors = vec![vec![1.0_f32, 2.0], vec![3.0_f32]];

        assert_eq!(mean_vector(&vectors), None);
    }

    /// A single, hand-built outline section shared by this module's
    /// `draft_and_revise_section` tests.
    fn section() -> OutlineSection {
        OutlineSection {
            node_id: NarrativeNodeId::default(),
            stage: MonomythStage::CallToAdventure,
            synopsis_hint: "a stranger arrives with a warning".to_owned(),
        }
    }

    #[tokio::test]
    async fn should_converge_immediately_when_the_first_attempt_already_meets_threshold() {
        let knowledge = in_memory_reference_knowledge();
        knowledge
            .ingest_reference(
                REFERENCE_SOURCE_ID,
                IngestInput::new("A stranger arrives bearing a warning.".to_owned()),
            )
            .await
            .expect("ingesting a declared reference source should succeed");

        let settings = ComposeSettings {
            revise_threshold: -1.0,
            max_revise_iterations: 3,
            ..ComposeSettings::default()
        };
        let backend = FakeBackend::scripted(vec![Continuation {
            text: "The stranger knocked at dusk.".to_owned(),
            is_complete: true,
        }]);
        let calls = backend.call_count();
        let llm = llm_with(backend);

        let drafted = draft_and_revise_section(&llm, &knowledge, &section(), "", &settings)
            .await
            .expect("drafting should succeed");

        assert_eq!(drafted.text, "The stranger knocked at dusk.");
        assert_eq!(drafted.note, None, "a converged section carries no note");
        assert_eq!(
            *calls.lock().expect("lock poisoned"),
            1,
            "a first attempt that already meets threshold must not trigger a revision"
        );
    }

    #[tokio::test]
    async fn should_keep_best_attempt_and_attach_a_note_when_threshold_is_never_met() {
        // Cosine similarity is always <= 1.0, so a threshold above 1.0 can
        // never be met: every attempt exhausts the revise loop.
        const IMPOSSIBLE_THRESHOLD: f64 = 2.0;
        const MAX_REVISE_ITERATIONS: usize = 2;

        let knowledge = in_memory_reference_knowledge();
        knowledge
            .ingest_reference(
                REFERENCE_SOURCE_ID,
                IngestInput::new("A stranger arrives bearing a warning.".to_owned()),
            )
            .await
            .expect("ingesting a declared reference source should succeed");

        let settings = ComposeSettings {
            revise_threshold: IMPOSSIBLE_THRESHOLD,
            max_revise_iterations: MAX_REVISE_ITERATIONS,
            ..ComposeSettings::default()
        };
        let backend = FakeBackend::scripted(vec![
            Continuation {
                text: "Attempt one.".to_owned(),
                is_complete: true,
            },
            Continuation {
                text: "Attempt two.".to_owned(),
                is_complete: true,
            },
            Continuation {
                text: "Attempt three.".to_owned(),
                is_complete: true,
            },
        ]);
        let calls = backend.call_count();
        let llm = llm_with(backend);

        let drafted = draft_and_revise_section(&llm, &knowledge, &section(), "", &settings)
            .await
            .expect("compose never errors on non-convergence");

        assert_eq!(
            *calls.lock().expect("lock poisoned"),
            MAX_REVISE_ITERATIONS + 1,
            "one initial attempt plus max_revise_iterations revisions"
        );
        assert!(
            drafted.note.is_some(),
            "a section that never converges must carry an explanatory note"
        );
        assert!(
            ["Attempt one.", "Attempt two.", "Attempt three."].contains(&drafted.text.as_str()),
            "the kept text must be one of the scripted attempts, got: {}",
            drafted.text
        );
    }

    #[tokio::test]
    async fn should_produce_a_single_unscored_draft_when_there_is_no_grounding() {
        let knowledge = in_memory_reference_knowledge();
        let settings = ComposeSettings {
            revise_threshold: 2.0,
            max_revise_iterations: 2,
            ..ComposeSettings::default()
        };
        let backend = FakeBackend::scripted(vec![Continuation {
            text: "The stranger knocked at dusk.".to_owned(),
            is_complete: true,
        }]);
        let calls = backend.call_count();
        let llm = llm_with(backend);

        let drafted = draft_and_revise_section(&llm, &knowledge, &section(), "", &settings)
            .await
            .expect("drafting should succeed");

        assert_eq!(drafted.text, "The stranger knocked at dusk.");
        assert_eq!(
            drafted.note, None,
            "scoring is skipped when there is no grounding to score against"
        );
        assert_eq!(
            *calls.lock().expect("lock poisoned"),
            1,
            "no grounding must skip the revise loop entirely"
        );
    }
}
