//! The Draft phase: narrating an [`Outline`]'s sections into prose, and the
//! end-to-end [`compose`] pipeline that wires Plan -> Draft -> Assemble together.
//!
//! Draft calls [`draft_and_revise_section`] once per [`OutlineSection`], threading
//! the prose drafted so far as `prior` so later sections stay coherent with
//! earlier ones. It never invents structure — the outline it narrates is a pure
//! function of the world, produced upstream by [`crate::plan::plan`]. Each
//! section is additionally grounded with REFERENCE-path passages retrieved from
//! [`monomyth_knowledge::Knowledge`] and scored/revised against that grounding;
//! see [`crate::revise`] for the licensing invariant and the revise loop.

use monomyth_core::World;
use monomyth_knowledge::Knowledge;
use monomyth_llm::Llm;

use crate::assemble::{LongFormDoc, SectionDraft, assemble};
use crate::error::ComposeError;
use crate::outline::Outline;
use crate::plan::plan;
use crate::revise::draft_and_revise_section;
use crate::settings::ComposeSettings;

/// The separator threading previously-drafted sections' prose into the next
/// section's `prior`, mirroring [`crate::assemble::assemble`]'s section separator.
const PRIOR_SEPARATOR: &str = "\n\n";

/// Draft every section of `outline` in spine order, threading each section's
/// prose into the `prior` of the sections that follow it.
///
/// Each section is grounded with REFERENCE-path passages retrieved from
/// `knowledge` before drafting; see the retrieval call below for the licensing
/// invariant.
///
/// `prior` grows with every section drafted, so prompt size grows across a long
/// spine; bounding or summarizing `prior` is a documented future concern, not
/// solved here.
///
/// # Errors
///
/// Returns [`ComposeError::Generation`] if any underlying [`generate_long_form`]
/// call fails, [`ComposeError::NoTurns`] if `settings.max_turns == 0`, or
/// [`ComposeError::Retrieval`] if a section's grounding retrieval fails. Never
/// errors for a section that fails to converge in the revise loop — see
/// [`draft_and_revise_section`].
///
/// [`generate_long_form`]: crate::generate::generate_long_form
pub async fn draft(
    llm: &Llm,
    knowledge: &Knowledge,
    outline: &Outline,
    settings: &ComposeSettings,
) -> Result<Vec<SectionDraft>, ComposeError> {
    let mut drafts = Vec::with_capacity(outline.len());
    let mut prior = String::new();

    for section in outline.sections() {
        let drafted = draft_and_revise_section(llm, knowledge, section, &prior, settings).await?;

        if !prior.is_empty() && !drafted.text.is_empty() {
            prior.push_str(PRIOR_SEPARATOR);
        }
        prior.push_str(&drafted.text);

        drafts.push(drafted);
    }

    Ok(drafts)
}

/// The end-to-end Plan -> Draft -> Assemble pipeline: reduce `world` to an
/// [`Outline`], narrate every section, then stitch the drafts into a
/// [`LongFormDoc`].
///
/// This is the deterministic pipeline skeleton plus reference-grounding
/// retrieval and the Revise feedback loop; any live/recorded cassette is a
/// later slice.
///
/// # Errors
///
/// Returns [`ComposeError::EmptyOutline`] if `world` has nothing composable, or
/// any error [`draft`] can return.
pub async fn compose(
    llm: &Llm,
    knowledge: &Knowledge,
    world: &World,
    settings: &ComposeSettings,
) -> Result<LongFormDoc, ComposeError> {
    let outline = plan(world)?;
    let sections = draft(llm, knowledge, &outline, settings).await?;
    Ok(assemble(sections))
}

#[cfg(test)]
mod tests {
    use monomyth_gen::Generator;
    use monomyth_knowledge::{IngestInput, Knowledge};

    use super::{compose, draft};
    use crate::outline::{Outline, OutlineSection};
    use crate::plan::plan;
    use crate::settings::ComposeSettings;
    use crate::test_support::{Continuation, FakeBackend, in_memory_reference_knowledge, llm_with};

    /// The seed used for the seeded-world `compose` test; arbitrary but fixed for
    /// determinism, matching `plan`'s test seed.
    const SEED: u64 = 42;

    /// A threshold so low that any drafted section's cosine score (always
    /// `>= -1.0`) converges on its first attempt, isolating these tests'
    /// assertions from the revise loop's own behavior (covered separately in
    /// `crate::revise`'s tests).
    fn always_converges_settings() -> ComposeSettings {
        ComposeSettings {
            revise_threshold: -1.0,
            ..ComposeSettings::default()
        }
    }

    /// A declared `reference`-namespace source id (see `corpus/manifest.json`),
    /// used to ingest reference passages for these tests' grounding.
    const REFERENCE_SOURCE_ID: &str = "perseus";

    /// A rare nonce token, ingested into the reference passage below, used to
    /// prove retrieved reference grounding actually reaches the drafting
    /// prompt (rather than merely being retrieved and discarded).
    const DISTINCTIVE_GROUNDING_TOKEN: &str = "Zephyrine";

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

    /// Build an [`in_memory_reference_knowledge`] layer with one reference
    /// passage ingested under [`REFERENCE_SOURCE_ID`], containing
    /// [`DISTINCTIVE_GROUNDING_TOKEN`] so tests can prove retrieval reached the
    /// prompt.
    async fn knowledge_with_reference_passage() -> Knowledge {
        let knowledge = in_memory_reference_knowledge();
        knowledge
            .ingest_reference(
                REFERENCE_SOURCE_ID,
                IngestInput::new(format!(
                    "A stranger named {DISTINCTIVE_GROUNDING_TOKEN} arrives bearing a warning \
                     that will not be refused twice."
                )),
            )
            .await
            .expect("ingesting a declared reference source should succeed");
        knowledge
    }

    #[tokio::test]
    async fn should_produce_one_section_draft_per_outline_section_in_order() {
        let outline = two_section_outline();
        let knowledge = knowledge_with_reference_passage().await;
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

        let drafts = draft(&llm, &knowledge, &outline, &always_converges_settings())
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
    async fn should_thread_reference_grounding_into_the_section_prompt() {
        let outline = two_section_outline();
        let knowledge = knowledge_with_reference_passage().await;
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
        let prompt_log = backend.prompt_log();
        let llm = llm_with(backend);

        draft(&llm, &knowledge, &outline, &always_converges_settings())
            .await
            .expect("drafting a two-section outline should succeed");

        let prompts = FakeBackend::prompts(&prompt_log);
        assert!(
            prompts
                .iter()
                .any(|prompt| prompt.contains(DISTINCTIVE_GROUNDING_TOKEN)),
            "retrieved reference grounding must reach at least one section's prompt; \
             captured prompts: {prompts:?}"
        );
    }

    #[tokio::test]
    async fn should_end_to_end_compose_seeded_world_into_ordered_matching_prose() {
        let world = Generator::with_default_passes()
            .generate_structure(SEED)
            .expect("the default pipeline generates a world");
        let outline = plan(&world).expect("a generated world is composable");
        let knowledge = knowledge_with_reference_passage().await;

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

        let doc = compose(&llm, &knowledge, &world, &always_converges_settings())
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
        let knowledge_a = knowledge_with_reference_passage().await;
        let knowledge_b = knowledge_with_reference_passage().await;

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

        let doc_a = compose(&llm_a, &knowledge_a, &world, &always_converges_settings())
            .await
            .expect("first compose call succeeds");
        let doc_b = compose(&llm_b, &knowledge_b, &world, &always_converges_settings())
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
