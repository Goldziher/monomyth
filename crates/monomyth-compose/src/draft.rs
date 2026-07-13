//! The Draft phase: narrating an [`Outline`]'s sections into prose, and the
//! end-to-end [`compose`] pipeline that wires Plan -> Draft -> Assemble together.
//!
//! Draft calls [`generate_long_form`] once per [`OutlineSection`], threading the
//! prose drafted so far as `prior` so later sections stay coherent with earlier
//! ones. It never invents structure — the outline it narrates is a pure function
//! of the world, produced upstream by [`crate::plan::plan`].

use monomyth_core::World;
use monomyth_llm::Llm;

use crate::assemble::{LongFormDoc, SectionDraft, assemble};
use crate::error::ComposeError;
use crate::generate::generate_long_form;
use crate::outline::{Outline, OutlineSection};
use crate::plan::plan;

/// The separator threading previously-drafted sections' prose into the next
/// section's `prior`, mirroring [`crate::assemble::assemble`]'s section separator.
const PRIOR_SEPARATOR: &str = "\n\n";

/// Build the per-section drafting instruction from a section's stage and
/// synopsis hint.
///
/// Kept as a simple, documented builder rather than a template abstraction — this
/// slice has exactly one prompt role (narrate this section). A template layer
/// arrives once the Draft phase needs multiple distinct roles (e.g. draft vs.
/// revise), per [`crate::generate`]'s prompt-role note.
fn section_instruction(section: &OutlineSection) -> String {
    format!(
        "Narrate the \"{stage}\" beat of the story. Grounding hint: {hint}",
        stage = section.stage.info().name,
        hint = section.synopsis_hint,
    )
}

/// Draft every section of `outline` in spine order, threading each section's
/// prose into the `prior` of the sections that follow it.
///
/// Retrieval grounding is `&[]` in this slice: per-section REFERENCE-path
/// grounding (via [`monomyth_knowledge::Knowledge`](../../monomyth_knowledge/index.html))
/// is wired in a later slice — this function deliberately takes no `Knowledge`
/// dependency yet.
///
/// `prior` grows with every section drafted, so prompt size grows across a long
/// spine; bounding or summarizing `prior` is a documented future concern, not
/// solved here.
///
/// # Errors
///
/// Returns [`ComposeError::Generation`] if any underlying [`generate_long_form`]
/// call fails, or [`ComposeError::NoTurns`] if `max_turns == 0`.
pub async fn draft(
    llm: &Llm,
    outline: &Outline,
    max_turns: usize,
) -> Result<Vec<SectionDraft>, ComposeError> {
    let mut drafts = Vec::with_capacity(outline.len());
    let mut prior = String::new();

    for section in outline.sections() {
        let instruction = section_instruction(section);
        let text = generate_long_form(llm, &instruction, &[], &prior, max_turns).await?;

        if !prior.is_empty() && !text.is_empty() {
            prior.push_str(PRIOR_SEPARATOR);
        }
        prior.push_str(&text);

        drafts.push(SectionDraft {
            node_id: section.node_id,
            stage: section.stage,
            text,
        });
    }

    Ok(drafts)
}

/// The end-to-end Plan -> Draft -> Assemble pipeline: reduce `world` to an
/// [`Outline`], narrate every section, then stitch the drafts into a
/// [`LongFormDoc`].
///
/// This is the deterministic pipeline skeleton only: the Revise/feedback loop,
/// live/recorded cassettes, and retrieval grounding are later slices.
///
/// # Errors
///
/// Returns [`ComposeError::EmptyOutline`] if `world` has nothing composable, or
/// any error [`draft`] can return.
pub async fn compose(
    llm: &Llm,
    world: &World,
    max_turns: usize,
) -> Result<LongFormDoc, ComposeError> {
    let outline = plan(world)?;
    let sections = draft(llm, &outline, max_turns).await?;
    Ok(assemble(sections))
}

#[cfg(test)]
mod tests {
    use monomyth_gen::Generator;

    use super::{compose, draft};
    use crate::outline::{Outline, OutlineSection};
    use crate::plan::plan;
    use crate::test_support::{Continuation, FakeBackend, llm_with};

    /// The seed used for the seeded-world `compose` test; arbitrary but fixed for
    /// determinism, matching `plan`'s test seed.
    const SEED: u64 = 42;

    /// Build a small, hand-constructed two-section outline for the `draft` unit
    /// test, avoiding a dependency on a generated world's exact shape.
    fn two_section_outline() -> Outline {
        use monomyth_core::NarrativeNodeId;
        use monomyth_frameworks::MonomythStage;

        Outline {
            sections: vec![
                OutlineSection {
                    node_id: NarrativeNodeId::default(),
                    stage: MonomythStage::CallToAdventure,
                    synopsis_hint: "a stranger arrives with a warning".to_owned(),
                },
                OutlineSection {
                    node_id: NarrativeNodeId::default(),
                    stage: MonomythStage::RefusalOfTheCall,
                    synopsis_hint: "she refuses it twice".to_owned(),
                },
            ],
        }
    }

    #[tokio::test]
    async fn should_produce_one_section_draft_per_outline_section_in_order() {
        let outline = two_section_outline();
        let script = vec![
            Continuation {
                text: "The village slept.".to_owned(),
                is_complete: true,
            },
            Continuation {
                text: "A stranger knocked.".to_owned(),
                is_complete: true,
            },
        ];
        let backend = FakeBackend::scripted(script);
        let calls = backend.call_count();
        let llm = llm_with(backend);

        let drafts = draft(&llm, &outline, 5)
            .await
            .expect("drafting a two-section outline should succeed");

        assert_eq!(
            drafts.len(),
            outline.len(),
            "one SectionDraft per outline section"
        );
        assert_eq!(drafts[0].node_id, outline.sections()[0].node_id);
        assert_eq!(drafts[0].stage, outline.sections()[0].stage);
        assert_eq!(drafts[0].text, "The village slept.");
        assert_eq!(drafts[1].node_id, outline.sections()[1].node_id);
        assert_eq!(drafts[1].stage, outline.sections()[1].stage);
        assert_eq!(drafts[1].text, "A stranger knocked.");
        assert_eq!(
            *calls.lock().expect("lock poisoned"),
            outline.len(),
            "each section must complete in exactly one turn, one backend call per section"
        );
    }

    #[tokio::test]
    async fn should_end_to_end_compose_seeded_world_into_ordered_matching_prose() {
        let world = Generator::with_default_passes()
            .generate_structure(SEED)
            .expect("the default pipeline generates a world");
        let outline = plan(&world).expect("a generated world is composable");

        let script: Vec<Continuation> = outline
            .sections()
            .iter()
            .enumerate()
            .map(|(index, _)| Continuation {
                text: format!("Section {index} prose."),
                is_complete: true,
            })
            .collect();
        let backend = FakeBackend::scripted(script);
        let llm = llm_with(backend);

        let doc = compose(&llm, &world, 5)
            .await
            .expect("composing a seeded world should succeed");

        assert_eq!(doc.sections().len(), outline.sections().len());

        let doc_ids: Vec<_> = doc
            .sections()
            .iter()
            .map(|section| section.node_id)
            .collect();
        let outline_ids: Vec<_> = outline
            .sections()
            .iter()
            .map(|section| section.node_id)
            .collect();
        assert_eq!(
            doc_ids, outline_ids,
            "drafted sections must match outline order"
        );

        let expected_prose = (0..outline.sections().len())
            .map(|index| format!("Section {index} prose."))
            .collect::<Vec<_>>()
            .join("\n\n");
        assert_eq!(doc.prose(), expected_prose);
    }

    #[tokio::test]
    async fn should_be_deterministic_across_identically_scripted_compose_runs() {
        let world = Generator::with_default_passes()
            .generate_structure(SEED)
            .expect("the default pipeline generates a world");
        let outline = plan(&world).expect("a generated world is composable");

        let build_script = || {
            outline
                .sections()
                .iter()
                .enumerate()
                .map(|(index, _)| Continuation {
                    text: format!("Section {index} prose."),
                    is_complete: true,
                })
                .collect::<Vec<_>>()
        };

        let llm_a = llm_with(FakeBackend::scripted(build_script()));
        let llm_b = llm_with(FakeBackend::scripted(build_script()));

        let doc_a = compose(&llm_a, &world, 5)
            .await
            .expect("first compose call succeeds");
        let doc_b = compose(&llm_b, &world, 5)
            .await
            .expect("second compose call succeeds");

        let json_a = serde_json::to_string(&doc_a).expect("LongFormDoc serializes");
        let json_b = serde_json::to_string(&doc_b).expect("LongFormDoc serializes");
        assert_eq!(
            json_a, json_b,
            "identically-scripted compose runs must serialize identically"
        );
    }
}
