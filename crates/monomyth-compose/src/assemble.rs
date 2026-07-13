//! The Assemble phase: stitching drafted section prose into one long-form document.
//!
//! Assemble is the final stage of the Plan -> Draft -> Assemble pipeline. It performs
//! no IO, no LLM calls, and no async work — it is a pure function of the drafted
//! sections, matching [`crate::plan::plan`]'s determinism.

use monomyth_core::NarrativeNodeId;
use monomyth_frameworks::MonomythStage;
use serde::{Deserialize, Serialize};

/// The separator joining drafted sections' prose in [`assemble`].
///
/// Mirrors [`crate::generate::generate_long_form`]'s turn separator: a blank line
/// keeps distinct sections visually distinct without merging mid-sentence.
const SECTION_SEPARATOR: &str = "\n\n";

/// One drafted outline section's prose.
///
/// Produced by [`crate::draft::draft`] — one per [`OutlineSection`](crate::OutlineSection)
/// the Draft phase narrated.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SectionDraft {
    /// The spine node this section covers.
    pub node_id: NarrativeNodeId,
    /// The node's primary Campbell stage.
    pub stage: MonomythStage,
    /// The drafted prose for this section.
    pub text: String,
    /// `None` when the section converged on (or above) its grounding-faithfulness
    /// threshold, or when scoring was skipped entirely (no grounding to score
    /// against). `Some(reason)` when the revise loop exhausted its attempt cap
    /// and this is the best-scoring attempt seen, kept below threshold; see
    /// [`crate::revise::draft_and_revise_section`].
    pub note: Option<String>,
}

/// The assembled long-form document: every drafted section plus the stitched
/// full text.
///
/// This is a **sidecar artifact** compose returns to its caller — it is not stored
/// in [`World`](monomyth_core::World) (per ADR-0027). Callers that want it persisted
/// alongside a world are responsible for their own storage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LongFormDoc {
    /// The drafted sections, in spine order.
    pub sections: Vec<SectionDraft>,
    /// The full text: every section's prose stitched together in order.
    pub prose: String,
}

impl LongFormDoc {
    /// The drafted sections, in spine order.
    #[must_use]
    pub fn sections(&self) -> &[SectionDraft] {
        &self.sections
    }

    /// The full assembled text.
    #[must_use]
    pub fn prose(&self) -> &str {
        &self.prose
    }
}

/// Stitch `sections`' prose into a [`LongFormDoc`], in order.
///
/// Non-empty section texts are joined by [`SECTION_SEPARATOR`]; empty section texts
/// contribute nothing to `prose` (no dangling separator either side), so a section
/// that drafted to nothing does not leave a blank gap in the stitched text.
#[must_use]
pub fn assemble(sections: Vec<SectionDraft>) -> LongFormDoc {
    let prose = sections
        .iter()
        .map(|section| section.text.as_str())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(SECTION_SEPARATOR);

    LongFormDoc { sections, prose }
}

#[cfg(test)]
mod tests {
    use monomyth_core::NarrativeNodeId;
    use monomyth_frameworks::MonomythStage;

    use super::{SectionDraft, assemble};

    /// Build a section carrying `text`, with the other fields fixed — the tests
    /// below assert only on `text` stitching, so node/stage/note are constant.
    fn section(text: &str) -> SectionDraft {
        SectionDraft {
            node_id: NarrativeNodeId::default(),
            stage: MonomythStage::CallToAdventure,
            text: text.to_owned(),
            note: None,
        }
    }

    #[test]
    fn should_join_non_empty_section_texts_with_blank_line_separator() {
        let sections = vec![
            section("The call arrives at dusk."),
            section("She refuses it twice."),
        ];

        let doc = assemble(sections.clone());

        assert_eq!(
            doc.prose(),
            "The call arrives at dusk.\n\nShe refuses it twice.",
            "sections must be stitched in order, separated by a blank line"
        );
        assert_eq!(
            doc.sections(),
            sections.as_slice(),
            "sections must round-trip unchanged"
        );
    }

    #[test]
    fn should_produce_empty_prose_for_no_sections() {
        let doc = assemble(Vec::new());

        assert_eq!(doc.prose(), "", "no sections stitch to empty prose");
        assert!(doc.sections().is_empty(), "no sections round-trip as none");
    }

    #[test]
    fn should_stitch_a_single_section_without_a_separator() {
        let sections = vec![section("The lone beat stands alone.")];

        let doc = assemble(sections.clone());

        assert_eq!(
            doc.prose(),
            "The lone beat stands alone.",
            "a single section carries no separator"
        );
        assert_eq!(doc.sections(), sections.as_slice());
    }

    #[test]
    fn should_produce_empty_prose_when_every_section_is_empty() {
        let sections = vec![section(""), section(""), section("")];

        let doc = assemble(sections.clone());

        assert_eq!(
            doc.prose(),
            "",
            "all-empty sections stitch to empty prose, no dangling separators"
        );
        assert_eq!(
            doc.sections(),
            sections.as_slice(),
            "the empty sections still round-trip unchanged"
        );
    }

    #[test]
    fn should_skip_empty_sections_without_leaving_a_blank_gap() {
        let sections = vec![
            section("The call arrives at dusk."),
            section(""),
            section("She crosses the threshold."),
        ];

        let doc = assemble(sections.clone());

        assert_eq!(
            doc.prose(),
            "The call arrives at dusk.\n\nShe crosses the threshold.",
            "an empty middle section drops out with no double separator"
        );
        assert_eq!(doc.sections(), sections.as_slice());
    }
}
