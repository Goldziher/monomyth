//! The story spine: the branching [`NarrativeStructure`] and the [`Quest`]s
//! hanging off it.
//!
//! The vocabulary here is not invented — it is bound to the framework artifacts
//! via `monomyth-frameworks`. Each [`NarrativeNode`](crate::NarrativeNode) carries a
//! Campbell stage anchor, and the story records the macro
//! [`BookerPlot`](monomyth_frameworks::BookerPlot) knob it was grown from, so the
//! structure stays grounded in the scholarship rather than drifting into ad-hoc
//! data.

use monomyth_frameworks::{BookerPlot, PoltiSituation};
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

use crate::content::Content;
use crate::ids::QuestId;
use crate::narrative::NarrativeStructure;
use crate::scored::ScoredOne;

/// A goal the player pursues, optionally grounded in a Polti dramatic situation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quest {
    /// The quest's title slot.
    pub title: Content,
    /// The dramatic situation the quest instantiates, if any, scored against
    /// weaker competing readings (ADR-0022). The generator assigns this directly
    /// (not from a weighted candidate draw), so alternatives are always empty
    /// today.
    pub situation: Option<ScoredOne<PoltiSituation>>,
    /// Whether the quest has been completed.
    pub complete: bool,
}

/// The story layer of a [`World`](crate::World): the branching narrative structure,
/// the macro plot it was grown from, and the quests.
///
/// Like [`World`](crate::World), `Story` holds [`SlotMap`]s (directly, and inside
/// [`NarrativeStructure`]) and so does not implement [`PartialEq`]; compare via the
/// serialized form.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Story {
    /// The branching narrative skeleton.
    pub structure: NarrativeStructure,
    /// The macro plot knob the structure was grown from, if recorded, scored
    /// against weaker competing readings (ADR-0022). The generator assigns this
    /// directly (not from a weighted candidate draw), so alternatives are always
    /// empty today.
    pub plot: Option<ScoredOne<BookerPlot>>,
    /// All quests, keyed by [`QuestId`].
    pub quests: SlotMap<QuestId, Quest>,
}
