//! The [`Outline`]: a pure, structural summary of a [`World`](monomyth_core::World)'s
//! narrative spine, ready for a later draft phase to narrate.

use monomyth_core::NarrativeNodeId;
use monomyth_frameworks::MonomythStage;
use serde::{Deserialize, Serialize};

/// One beat of the spine, reduced to what a draft phase needs to narrate it.
///
/// `synopsis_hint` is read from the node's synopsis [`Content`](monomyth_core::Content)
/// *prompt* hint, never its filled value. This keeps the outline a pure function of
/// structure: it is identical whether the content phase has run or not, so a plan
/// built before content generation and one built after are interchangeable, and
/// planning never has to wait on (or depend on the non-determinism of) the LLM
/// content pass.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutlineSection {
    /// The spine node this section covers.
    pub node_id: NarrativeNodeId,
    /// The node's primary Campbell stage.
    pub stage: MonomythStage,
    /// The grounding hint from the node's synopsis prompt.
    pub synopsis_hint: String,
}

/// The full spine, reduced to an ordered sequence of [`OutlineSection`]s.
///
/// Produced by [`plan`](crate::plan) and consumed by the (future) draft phase: the
/// LLM narrates this structure but cannot invent it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outline {
    /// The sections, in spine order.
    pub sections: Vec<OutlineSection>,
}

impl Outline {
    /// The number of sections.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sections.len()
    }

    /// Whether the outline has no sections.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }

    /// The sections, in spine order.
    #[must_use]
    pub fn sections(&self) -> &[OutlineSection] {
        &self.sections
    }
}
