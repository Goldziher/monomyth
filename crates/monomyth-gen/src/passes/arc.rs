//! [`ArcPass`]: build the story spine from the framework artifacts.
//!
//! The arc is the full seventeen-stage Campbell monomyth, one
//! [`StageBeat`](monomyth_core::StageBeat) per [`MonomythStage`] in artifact `id`
//! order (deterministic, and grounded in scholarship rather than invented). Each
//! synopsis slot is left empty, hinted with the stage and the Propp functions that
//! realize it. The pass also grounds one or two quests in a seed-chosen Booker
//! plot via its Polti situations.

use monomyth_core::{Content, ContentKind, ContentPrompt, Quest, StageBeat, World};
use monomyth_frameworks::{BookerPlot, MonomythStage, arc_functions, plot_situations};
use rand_chacha::ChaCha8Rng;

use crate::error::GenError;
use crate::pass::{ProceduralPass, draw_range_inclusive};

/// The fewest quests the arc pass grounds in the chosen plot.
const MIN_QUESTS: usize = 1;
/// The most quests the arc pass grounds in the chosen plot.
const MAX_QUESTS: usize = 2;

/// Builds the monomyth arc, sets the opening stage, and seeds quests.
#[derive(Debug, Default, Clone, Copy)]
pub struct ArcPass;

impl ArcPass {
    /// Construct the arc pass.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ProceduralPass for ArcPass {
    fn name(&self) -> &'static str {
        "ArcPass"
    }

    fn apply(&self, world: &mut World, rng: &mut ChaCha8Rng) -> Result<(), GenError> {
        let stages = MonomythStage::all();

        world.story.arc = stages
            .iter()
            .enumerate()
            .map(|(index, &stage)| {
                let hint = format!(
                    "monomyth stage {stage:?} ({} of {}); realizes Propp functions {:?}",
                    index + 1,
                    stages.len(),
                    arc_functions(stage),
                );
                StageBeat::new(
                    stage,
                    Content::empty(ContentPrompt::new(ContentKind::Synopsis, hint)),
                )
            })
            .collect();

        // The opening stage is the first beat of the arc.
        let opening = stages
            .first()
            .copied()
            .ok_or(GenError::Invariant("the monomyth arc has no stages"))?;
        world.story.current_stage = opening;

        // Ground a small number of quests in one seed-chosen Booker plot.
        let plots = BookerPlot::all();
        // Guard the `len() - 1` against underflow, mirroring the situations guard
        // below; a non-empty plot set leaves the draw sequence unchanged.
        if plots.is_empty() {
            return Err(GenError::Invariant("the Booker plot set is empty"));
        }
        let plot = plots
            .get(draw_range_inclusive(rng, 0, plots.len() - 1))
            .copied()
            .ok_or(GenError::Invariant("Booker plot index out of range"))?;
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

        Ok(())
    }
}
