//! [`BackbonePass`]: build the branching narrative skeleton from the framework
//! artifacts.
//!
//! This is the macro tier of narrative generation. It lays a trunk of one
//! [`NarrativeNode`] per [`MonomythStage`] in artifact `id` order (deterministic,
//! grounded in scholarship rather than invented), chained by [`EdgeKind::Sequence`]
//! edges — the spine. At the corpus-flagged **optional** stages
//! ([`MonomythStage::info`]`().optional`) it opens **player-choice fork diamonds**:
//! the mandatory spine skips the optional beat via its primary `Sequence` edge,
//! while a [`EdgeKind::Choice`] edge detours through it and reconverges on the next
//! mandatory stage. Because consecutive optional stages exist, optionals are grouped
//! into *runs* and each run is one detour; forks add edges, not nodes, so the node
//! count is fixed at the number of stages and the graph is always a reconverging DAG.
//!
//! Whether a run forks is a seeded [`draw_chance`] over
//! [`NarrativeConfig::fork_chance_permille`]; the roll is taken once per run
//! regardless of outcome, so the stream position never depends on graph shape.
//!
//! The pass also records the seed-chosen [`BookerPlot`] on the story and grounds one
//! or two quests in that plot via its Polti situations. Every prose slot (node
//! synopsis, edge label) is left empty for a later content layer.

use std::collections::{BTreeMap, BTreeSet};

use monomyth_core::{
    Content, ContentKind, ContentPrompt, EdgeKind, NarrativeEdge, NarrativeNode,
    NarrativeStructure, NodeKind, Quest, World,
};
use monomyth_frameworks::{BookerPlot, MonomythStage, arc_functions, plot_situations};
use rand_chacha::ChaCha8Rng;

use crate::error::GenError;
use crate::pass::{ProceduralPass, draw_chance, draw_range_inclusive};

/// The fewest quests the backbone pass grounds in the chosen plot.
const MIN_QUESTS: usize = 1;
/// The most quests the backbone pass grounds in the chosen plot.
const MAX_QUESTS: usize = 2;
/// Default probability (permille) that an optional-stage run forks into a choice.
const DEFAULT_FORK_CHANCE_PERMILLE: u16 = 500;

/// How the macro [`BookerPlot`] backbone is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlotChoice {
    /// Draw the plot from the seed.
    Seeded,
    /// Use a fixed plot, consuming no randomness.
    Fixed(BookerPlot),
}

/// Tunable knobs for narrative-structure generation (the macro layer).
///
/// v1 exposes the macro controls. Branch factor, a node budget, a reconvergence
/// toggle, and the meso/micro tier switches are deferred to later layers, which is
/// why they are absent here rather than present-but-ignored.
#[derive(Debug, Clone, Copy)]
pub struct NarrativeConfig {
    /// How the Booker plot is chosen.
    pub plot: PlotChoice,
    /// Probability, in permille (0..=1000), that a run of optional stages forks
    /// into a player choice rather than being taken inline on the spine.
    pub fork_chance_permille: u16,
}

impl Default for NarrativeConfig {
    fn default() -> Self {
        Self {
            plot: PlotChoice::Seeded,
            fork_chance_permille: DEFAULT_FORK_CHANCE_PERMILLE,
        }
    }
}

/// A maximal run of consecutive optional stages, by index into
/// [`MonomythStage::all`]. Its mandatory anchor is `first - 1` and its
/// reconvergence target is `last + 1`; both are guaranteed to exist because the
/// first and last Campbell stages are mandatory.
#[derive(Debug, Clone, Copy)]
struct Run {
    first: usize,
    last: usize,
}

/// Builds the branching narrative structure, positions the play cursor, records the
/// macro plot, and seeds quests.
#[derive(Debug, Default, Clone, Copy)]
pub struct BackbonePass {
    config: NarrativeConfig,
}

impl BackbonePass {
    /// Construct the backbone pass with the default [`NarrativeConfig`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct the backbone pass with an explicit configuration.
    #[must_use]
    pub fn with_config(config: NarrativeConfig) -> Self {
        Self { config }
    }
}

impl ProceduralPass for BackbonePass {
    fn name(&self) -> &'static str {
        "BackbonePass"
    }

    fn apply(&self, world: &mut World, rng: &mut ChaCha8Rng) -> Result<(), GenError> {
        // Choose the macro plot first; the fork rolls follow in a fixed order, so
        // the stream position is independent of the resulting graph shape.
        let plot = match self.config.plot {
            PlotChoice::Fixed(plot) => plot,
            PlotChoice::Seeded => {
                let plots = BookerPlot::all();
                if plots.is_empty() {
                    return Err(GenError::Invariant("the Booker plot set is empty"));
                }
                plots
                    .get(draw_range_inclusive(rng, 0, plots.len() - 1))
                    .copied()
                    .ok_or(GenError::Invariant("Booker plot index out of range"))?
            }
        };

        // One fork decision per optional-stage run, in id order (deterministic).
        let runs = optional_runs()?;
        let decisions: Vec<bool> = runs
            .iter()
            .map(|_| draw_chance(rng, self.config.fork_chance_permille))
            .collect();

        let structure = build_structure(&runs, &decisions)?;
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

/// The maximal runs of consecutive optional stages in `MonomythStage::all()` order.
///
/// # Errors
///
/// Returns [`GenError::Invariant`] if a run has no mandatory anchor before it or no
/// mandatory stage after it (which would mean the first or last Campbell stage is
/// optional — a corrupt framework artifact).
fn optional_runs() -> Result<Vec<Run>, GenError> {
    let stages = MonomythStage::all();
    let mut runs = Vec::new();
    let mut index = 0;
    while index < stages.len() {
        if !stages[index].info().optional {
            index += 1;
            continue;
        }
        let first = index;
        let mut last = index;
        while last + 1 < stages.len() && stages[last + 1].info().optional {
            last += 1;
        }
        if first == 0 || last + 1 >= stages.len() {
            return Err(GenError::Invariant(
                "an optional stage run has no mandatory anchor",
            ));
        }
        runs.push(Run { first, last });
        index = last + 1;
    }
    Ok(runs)
}

/// A fresh empty choice-prose slot hinted with `hint`, filled by a later layer.
fn choice_slot(hint: &str) -> Content {
    Content::empty(ContentPrompt::new(ContentKind::Choice, hint))
}

/// Assemble the stage nodes and wire the spine plus one detour diamond per forked
/// run. Forks add edges only, never nodes, so the graph is a reconverging DAG whose
/// node count equals the stage count.
fn build_structure(runs: &[Run], decisions: &[bool]) -> Result<NarrativeStructure, GenError> {
    let stages = MonomythStage::all();
    let last = stages
        .len()
        .checked_sub(1)
        .ok_or(GenError::Invariant("the monomyth arc has no stages"))?;

    // Build via a default structure so the pass need not name the `slotmap` crate
    // directly (it is a transitive dependency through the model).
    let mut structure = NarrativeStructure::default();
    let mut ids = Vec::with_capacity(stages.len());
    for (index, &stage) in stages.iter().enumerate() {
        let hint = format!(
            "monomyth stage {stage:?} ({} of {}); realizes Propp functions {:?}",
            index + 1,
            stages.len(),
            arc_functions(stage),
        );
        let node = NarrativeNode::new(
            format!("{stage:?}"),
            NodeKind::Beat,
            stage,
            Content::empty(ContentPrompt::new(ContentKind::Synopsis, hint)),
        );
        ids.push(structure.nodes.insert(node));
    }

    // The anchors of forked runs, mapping each anchor's stage index to the index of
    // the mandatory stage the detour reconverges on.
    let forked: BTreeMap<usize, usize> = runs
        .iter()
        .zip(decisions)
        .filter_map(|(run, &take)| take.then_some((run.first - 1, run.last + 1)))
        .collect();

    // Wire each node's out-edges: a forked anchor gets a primary Sequence "skip"
    // edge to the reconvergence point plus a Choice edge into the optional run; every
    // other node advances linearly.
    for index in 0..stages.len() {
        if index == last {
            continue; // the final stage is the sole ending: no out-edge
        }
        let from = ids[index];
        if let Some(&after) = forked.get(&index) {
            let optional_stage = structure.nodes[ids[index + 1]].stage;
            let skip_hint = format!("skip the optional {optional_stage:?} arc");
            structure.nodes[from].out.push(NarrativeEdge::new(
                ids[after],
                EdgeKind::Sequence,
                choice_slot(&skip_hint),
            ));
            let enter_hint = format!("enter the optional {optional_stage:?} arc");
            structure.nodes[from].out.push(NarrativeEdge::new(
                ids[index + 1],
                EdgeKind::Choice,
                choice_slot(&enter_hint),
            ));
        } else {
            let to = ids[index + 1];
            let hint = format!(
                "advance from {:?} to {:?}",
                structure.nodes[from].stage, structure.nodes[to].stage,
            );
            structure
                .nodes
                .get_mut(from)
                .ok_or(GenError::Invariant("stage node missing during wiring"))?
                .out
                .push(NarrativeEdge::new(
                    to,
                    EdgeKind::Sequence,
                    choice_slot(&hint),
                ));
        }
    }

    // Set root/endings first so `recompute_kinds` can read them: it reads
    // `self.root` and each node's out-degree to derive kinds, matching the fixed
    // precedence (Origin for the root, Ending at out-degree 0, Branch at
    // out-degree > 1, Merge at in-degree > 1, else Beat).
    structure.root = ids[0];
    structure.endings = BTreeSet::from([ids[last]]);
    structure.recompute_kinds();
    Ok(structure)
}
