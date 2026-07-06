//! The story spine: the [`MonomythStage`] arc, its per-stage [`StageBeat`]s, and
//! the [`Quest`]s hanging off it.
//!
//! The vocabulary here is not invented — it is bound to the framework artifacts
//! via `monomyth-frameworks`. A [`StageBeat`] derives its Propp
//! [`functions`](StageBeat::functions) from
//! [`arc_functions`](monomyth_frameworks::arc_functions), so the structural beats
//! stay in lockstep with the scholarship rather than drifting into ad-hoc data.

use monomyth_frameworks::{MonomythStage, PoltiSituation, ProppFunction, arc_functions};
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

use crate::content::Content;
use crate::ids::QuestId;

/// One stage of the hero's-journey arc: the Campbell stage and a prose synopsis
/// slot. Its Propp functions are derived from the crosswalk on demand via
/// [`functions`](StageBeat::functions), never stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageBeat {
    /// The Campbell stage this beat covers.
    pub stage: MonomythStage,
    /// The beat's synopsis slot.
    pub synopsis: Content,
}

impl StageBeat {
    /// Build a beat for `stage` with its `synopsis` slot.
    ///
    /// ```
    /// use monomyth_core::{ContentKind, ContentPrompt, Content, StageBeat};
    /// use monomyth_frameworks::{arc_functions, MonomythStage};
    ///
    /// let stage = MonomythStage::CallToAdventure;
    /// let beat = StageBeat::new(stage, Content::empty(ContentPrompt::new(ContentKind::Synopsis, "")));
    /// assert_eq!(beat.functions(), arc_functions(stage));
    /// ```
    #[must_use]
    pub fn new(stage: MonomythStage, synopsis: Content) -> Self {
        Self { stage, synopsis }
    }

    /// The Propp functions that realize this beat's stage.
    ///
    /// Computed from [`arc_functions`](monomyth_frameworks::arc_functions) rather
    /// than stored, so the crosswalk artifacts remain the single source of truth:
    /// the functions can never drift from the scholarship and are absent from the
    /// serialized world.
    #[must_use]
    pub fn functions(&self) -> &'static [ProppFunction] {
        arc_functions(self.stage)
    }
}

/// A goal the player pursues, optionally grounded in a Polti dramatic situation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quest {
    /// The quest's title slot.
    pub title: Content,
    /// The dramatic situation the quest instantiates, if any.
    pub situation: Option<PoltiSituation>,
    /// Whether the quest has been completed.
    pub complete: bool,
}

/// The story layer of a [`World`](crate::World): the ordered arc, the stage the
/// player is currently in, and the quests.
///
/// Like [`World`](crate::World), `Story` holds a [`SlotMap`] and so does not
/// implement [`PartialEq`]; compare via the serialized form.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Story {
    /// The ordered hero's-journey beats.
    pub arc: Vec<StageBeat>,
    /// The stage the player currently occupies.
    pub current_stage: MonomythStage,
    /// All quests, keyed by [`QuestId`].
    pub quests: SlotMap<QuestId, Quest>,
}
