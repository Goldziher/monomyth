//! The template layer for every LLM prompt role the compose pipeline builds.
//!
//! Compose has three distinct prompt roles today: drafting a section
//! ([`section_draft_prompt`]), revising a section that fell short of its
//! grounding threshold ([`section_revise_prompt`]), and continuing prose
//! within a single [`crate::generate::generate_long_form`] call
//! ([`continuation_prompt`]). Each is tagged with a stable [`PromptRole`]
//! identifier so callers (tracing, cassette recording) can key on *which*
//! prompt role produced a given call without parsing prompt text.
//!
//! [`grounding_query`] lives here too, alongside the prompt builders it feeds,
//! even though it is not itself an LLM prompt — see its own doc comment for
//! the distinction.

use crate::outline::OutlineSection;

/// The stable identifier for [`PromptRole::SectionDraft`].
const SECTION_DRAFT_IDENTIFIER: &str = "compose.section_draft";

/// The stable identifier for [`PromptRole::SectionRevise`].
const SECTION_REVISE_IDENTIFIER: &str = "compose.section_revise";

/// The stable identifier for [`PromptRole::Continuation`].
const CONTINUATION_IDENTIFIER: &str = "compose.continuation";

/// A named prompt role in the compose pipeline.
///
/// Each variant identifies one distinct kind of LLM call the pipeline makes.
/// [`PromptRole::identifier`] returns a stable slug for that role, used for
/// tracing and cassette keys: changing a slug breaks trace/cassette
/// provenance and must be treated as a breaking change.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PromptRole {
    /// Narrating an outline section into prose for the first time.
    SectionDraft,
    /// Revising a previously-drafted section that fell short of its
    /// grounding-faithfulness threshold.
    SectionRevise,
    /// Continuing prose within a single multi-turn long-form generation call.
    Continuation,
}

impl PromptRole {
    /// The stable slug identifying this prompt role, for tracing and cassette
    /// keys.
    ///
    /// These identifiers are used for tracing and cassette keys; changing a
    /// slug breaks trace/cassette provenance and must be treated as a
    /// breaking change.
    #[must_use]
    pub const fn identifier(self) -> &'static str {
        match self {
            Self::SectionDraft => SECTION_DRAFT_IDENTIFIER,
            Self::SectionRevise => SECTION_REVISE_IDENTIFIER,
            Self::Continuation => CONTINUATION_IDENTIFIER,
        }
    }
}

/// Build the per-section drafting instruction from a section's stage and
/// synopsis hint.
///
/// Prompt role: [`PromptRole::SectionDraft`].
pub(crate) fn section_draft_prompt(section: &OutlineSection) -> String {
    format!(
        "Narrate the \"{stage}\" beat of the story. Grounding hint: {hint}",
        stage = section.stage.info().name,
        hint = section.synopsis_hint,
    )
}

/// Build the instruction for a revision attempt: tell the model the previous
/// attempt fell short of the grounding-faithfulness threshold and to stay
/// closer to the grounding this time.
///
/// Composes on [`section_draft_prompt`] as its base, so a revision always
/// carries the same beat/hint framing as the original draft.
///
/// Prompt role: [`PromptRole::SectionRevise`].
pub(crate) fn section_revise_prompt(
    section: &OutlineSection,
    previous: &str,
    score: f64,
    threshold: f64,
) -> String {
    format!(
        "{base}\n\nThe previous attempt scored {score:.3} against the grounding-faithfulness \
         threshold of {threshold:.3} and fell short. Revise the prose so it stays closer to the \
         grounding passages while still narrating the same beat. Previous attempt:\n{previous}",
        base = section_draft_prompt(section),
    )
}

/// Build the prompt for one turn: the instruction, the grounding passages
/// (labelled and joined), and the prose accumulated so far.
///
/// Prompt role: [`PromptRole::Continuation`].
pub(crate) fn continuation_prompt(
    instruction: &str,
    grounding: &[String],
    prior: &str,
    generated_so_far: &str,
) -> String {
    use std::fmt::Write as _;

    let mut prompt = String::new();
    let _ = writeln!(prompt, "Instruction: {instruction}");

    if !grounding.is_empty() {
        prompt.push_str("\nGrounding:\n");
        for passage in grounding {
            let _ = writeln!(prompt, "- {passage}");
        }
    }

    let so_far = combined_prose(prior, generated_so_far);
    if so_far.is_empty() {
        prompt.push_str("\nProse so far: (none; this is the opening turn)\n");
    } else {
        let _ = writeln!(prompt, "\nProse so far:\n{so_far}");
    }

    prompt.push_str(
        "\nContinue the prose from where it leaves off. Report the new text you are adding \
         (not the prose so far) and whether the section is now complete.",
    );
    prompt
}

/// Join `prior` and `generated_so_far` for display in the prompt, treating
/// either half as optional.
fn combined_prose(prior: &str, generated_so_far: &str) -> String {
    match (prior.is_empty(), generated_so_far.is_empty()) {
        (true, true) => String::new(),
        (true, false) => generated_so_far.to_owned(),
        (false, true) => prior.to_owned(),
        (false, false) => format!("{prior}\n\n{generated_so_far}"),
    }
}

/// Build the retrieval query text for `section`'s reference-grounding lookup.
///
/// This is a vector-search query string, **not** an LLM prompt — it is aimed
/// at [`monomyth_knowledge::Knowledge`]'s retrieval, not at a model. It
/// therefore deliberately has no [`PromptRole`]. It happens to share its two
/// source fields with [`section_draft_prompt`], but the two are conceptually
/// distinct: a retrieval query is a short, keyword-ish string, while a
/// drafting instruction is a narration directive. They stay separate helpers
/// rather than being folded together.
pub(crate) fn grounding_query(section: &OutlineSection) -> String {
    format!(
        "{stage}: {hint}",
        stage = section.stage.info().name,
        hint = section.synopsis_hint,
    )
}

#[cfg(test)]
mod tests {
    use monomyth_core::NarrativeNodeId;
    use monomyth_frameworks::MonomythStage;

    use super::{
        CONTINUATION_IDENTIFIER, PromptRole, SECTION_DRAFT_IDENTIFIER, SECTION_REVISE_IDENTIFIER,
        continuation_prompt, section_revise_prompt,
    };
    use crate::outline::OutlineSection;

    /// A single, hand-built outline section shared by this module's tests.
    fn section() -> OutlineSection {
        OutlineSection {
            node_id: NarrativeNodeId::default(),
            stage: MonomythStage::CallToAdventure,
            synopsis_hint: "a stranger arrives with a warning".to_owned(),
        }
    }

    #[test]
    fn should_return_stable_slug_for_section_draft_role() {
        assert_eq!(
            PromptRole::SectionDraft.identifier(),
            SECTION_DRAFT_IDENTIFIER
        );
        assert_eq!(
            PromptRole::SectionDraft.identifier(),
            "compose.section_draft"
        );
    }

    #[test]
    fn should_return_stable_slug_for_section_revise_role() {
        assert_eq!(
            PromptRole::SectionRevise.identifier(),
            SECTION_REVISE_IDENTIFIER
        );
        assert_eq!(
            PromptRole::SectionRevise.identifier(),
            "compose.section_revise"
        );
    }

    #[test]
    fn should_return_stable_slug_for_continuation_role() {
        assert_eq!(
            PromptRole::Continuation.identifier(),
            CONTINUATION_IDENTIFIER
        );
        assert_eq!(
            PromptRole::Continuation.identifier(),
            "compose.continuation"
        );
    }

    #[test]
    fn should_thread_grounding_passage_into_continuation_prompt() {
        const DISTINCTIVE_GROUNDING_TOKEN: &str = "Zephyrine";
        let grounding = vec![format!(
            "A stranger named {DISTINCTIVE_GROUNDING_TOKEN} arrives."
        )];

        let prompt = continuation_prompt("narrate the departure", &grounding, "", "");

        assert!(
            prompt.contains(DISTINCTIVE_GROUNDING_TOKEN),
            "grounding passages must reach the continuation prompt; got: {prompt}"
        );
    }

    #[test]
    fn should_include_previous_attempt_and_score_and_threshold_in_revise_prompt() {
        const PREVIOUS_ATTEMPT: &str = "The stranger arrived, but too quietly.";
        const SCORE: f64 = 0.412_345;
        const THRESHOLD: f64 = 0.75;

        let prompt = section_revise_prompt(&section(), PREVIOUS_ATTEMPT, SCORE, THRESHOLD);

        assert!(
            prompt.contains(PREVIOUS_ATTEMPT),
            "the previous attempt text must appear in the revise prompt; got: {prompt}"
        );
        assert!(
            prompt.contains("0.412"),
            "the score must appear formatted to three decimal places; got: {prompt}"
        );
        assert!(
            prompt.contains("0.750"),
            "the threshold must appear formatted to three decimal places; got: {prompt}"
        );
    }
}
