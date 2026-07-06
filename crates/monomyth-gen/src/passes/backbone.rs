//! [`BackbonePass`]: build the branching narrative skeleton from the framework
//! artifacts.
//!
//! This is the macro tier of narrative generation. In this milestone the backbone
//! is a **linear trunk**: one [`NarrativeNode`] per [`MonomythStage`] in artifact
//! `id` order (deterministic, grounded in scholarship rather than invented),
//! chained by a single [`EdgeKind::Sequence`] edge each. The first stage node is
//! the [`root`](NarrativeStructure::root) (a [`NodeKind::Origin`]); the last is the
//! sole ending (a [`NodeKind::Ending`]).
//!
//! Player-choice fork diamonds at the corpus-flagged *optional* stages are the next
//! milestone; the node/edge model already accommodates them (the empty edge labels
//! and node guards are the forward seams), so this pass builds the fork-less spine
//! without foreclosing them.
//!
//! Each synopsis slot is left empty, hinted with the stage and the Propp functions
//! that realize it. The pass also records the seed-chosen [`BookerPlot`] on the
//! story and grounds one or two quests in that plot via its Polti situations.

use std::collections::BTreeSet;

use monomyth_core::{
    Content, ContentKind, ContentPrompt, EdgeKind, NarrativeEdge, NarrativeNode,
    NarrativeStructure, NodeKind, Quest, World,
};
use monomyth_frameworks::{BookerPlot, MonomythStage, arc_functions, plot_situations};
use rand_chacha::ChaCha8Rng;

use crate::error::GenError;
use crate::pass::{ProceduralPass, draw_range_inclusive};

/// The fewest quests the backbone pass grounds in the chosen plot.
const MIN_QUESTS: usize = 1;
/// The most quests the backbone pass grounds in the chosen plot.
const MAX_QUESTS: usize = 2;

/// Builds the branching narrative structure, positions the play cursor, records the
/// macro plot, and seeds quests.
#[derive(Debug, Default, Clone, Copy)]
pub struct BackbonePass;

impl BackbonePass {
    /// Construct the backbone pass.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    /// Assemble the linear trunk of stage nodes into a [`NarrativeStructure`].
    ///
    /// Nodes are inserted in [`MonomythStage`] `id` order, so the `SlotMap` insertion
    /// order — and therefore the serialized output — is deterministic. The trunk
    /// makes no RNG draws, so it does not perturb the pass's stream position.
    fn build_structure() -> Result<NarrativeStructure, GenError> {
        let stages = MonomythStage::all();
        let last = stages
            .len()
            .checked_sub(1)
            .ok_or(GenError::Invariant("the monomyth arc has no stages"))?;

        // Build via a default structure so the pass need not name the `slotmap`
        // crate directly (it is a transitive dependency through the model).
        let mut structure = NarrativeStructure::default();
        let mut ids = Vec::with_capacity(stages.len());
        for (index, &stage) in stages.iter().enumerate() {
            let kind = if index == 0 {
                NodeKind::Origin
            } else if index == last {
                NodeKind::Ending
            } else {
                NodeKind::Beat
            };
            let hint = format!(
                "monomyth stage {stage:?} ({} of {}); realizes Propp functions {:?}",
                index + 1,
                stages.len(),
                arc_functions(stage),
            );
            let node = NarrativeNode::new(
                format!("{stage:?}"),
                kind,
                stage,
                Content::empty(ContentPrompt::new(ContentKind::Synopsis, hint)),
            );
            ids.push(structure.nodes.insert(node));
        }

        // Chain each node to the next with a single Sequence edge.
        for pair in ids.windows(2) {
            let [from, to] = [pair[0], pair[1]];
            let hint = format!(
                "advance from {:?} to {:?}",
                structure.nodes[from].stage, structure.nodes[to].stage,
            );
            structure.nodes[from].out.push(NarrativeEdge::new(
                to,
                EdgeKind::Sequence,
                Content::empty(ContentPrompt::new(ContentKind::Choice, hint)),
            ));
        }

        structure.root = *ids
            .first()
            .ok_or(GenError::Invariant("the monomyth arc has no stages"))?;
        let ending = *ids
            .last()
            .ok_or(GenError::Invariant("the monomyth arc has no stages"))?;
        structure.endings = BTreeSet::from([ending]);
        Ok(structure)
    }
}

impl ProceduralPass for BackbonePass {
    fn name(&self) -> &'static str {
        "BackbonePass"
    }

    fn apply(&self, world: &mut World, rng: &mut ChaCha8Rng) -> Result<(), GenError> {
        // Choose the macro plot first so the RNG position is independent of graph
        // shape; the fork-less trunk itself makes no draws.
        let plots = BookerPlot::all();
        if plots.is_empty() {
            return Err(GenError::Invariant("the Booker plot set is empty"));
        }
        let plot = plots
            .get(draw_range_inclusive(rng, 0, plots.len() - 1))
            .copied()
            .ok_or(GenError::Invariant("Booker plot index out of range"))?;

        let structure = Self::build_structure()?;
        structure.validate()?;
        let root = structure.root();

        // Ground a small number of quests in the chosen plot's Polti situations.
        let situations = plot_situations(plot);
        let quest_count = draw_range_inclusive(rng, MIN_QUESTS, MAX_QUESTS);
        for quest_index in 0..quest_count {
            let situation = if situations.is_empty() {
                None
            } else {
                situations
                    .get(draw_range_inclusive(rng, 0, situations.len() - 1))
                    .copied()
            };
            let hint = format!(
                "quest {} of {quest_count} for plot {plot:?}, situation {situation:?}",
                quest_index + 1,
            );
            world.story.quests.insert(Quest {
                title: Content::empty(ContentPrompt::new(ContentKind::Title, hint)),
                situation,
                complete: false,
            });
        }

        world.story.structure = structure;
        world.story.plot = Some(plot);
        world.state.cursor = root;
        Ok(())
    }
}
