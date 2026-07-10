//! [`BeatPass`]: grow the narrative to a playable length by expanding each Campbell
//! stage node into a short chain of grounded beats.
//!
//! This is the **meso length** tier of narrative generation, sitting between the
//! macro [`BackbonePass`](crate::BackbonePass) (one node per stage) and the future
//! micro content layer. The backbone lays down a spine of roughly a dozen-and-a-half
//! stage nodes; a real adventure needs more moment-to-moment structure than that.
//! This pass walks every existing node and splices a seed-chosen number of extra
//! beats onto its primary spine (`Sequence`) edge, so a single stage becomes
//! `stage → beat_1 → … → beat_k → next`.
//!
//! The growth is driven entirely through the narrative *edit surface*
//! ([`NarrativeEdit::InsertBeat`]): the pass never touches the [`SlotMap`] or edge
//! vectors directly, so every mutation is an inspectable, validated command. Each new
//! beat inherits its parent's Campbell [`stage`](monomyth_frameworks::MonomythStage)
//! and is grounded with one realized Propp function drawn from
//! [`arc_functions`](monomyth_frameworks::arc_functions) — the scholarship, not
//! invention. Prose slots (node synopsis) stay empty for the later content layer.

use std::collections::BTreeSet;

use monomyth_core::{EdgeKind, EditOutcome, NarrativeEdit, NarrativeNodeId, NodeSpec, World};
use monomyth_frameworks::{MonomythStage, arc_functions};
use rand_chacha::ChaCha8Rng;

use crate::error::GenError;
use crate::pass::{ProceduralPass, draw_range_inclusive};

/// The fewest beats the pass splices onto each stage node's spine.
const DEFAULT_BEATS_MIN: usize = 1;
/// The most beats the pass splices onto each stage node's spine.
const DEFAULT_BEATS_MAX: usize = 3;

/// Tunable knobs for beat expansion (the meso length layer).
///
/// A stage node is expanded into `beats_per_stage_min..=beats_per_stage_max` extra
/// beats, drawn per node from the pass's sub-stream. The invariant `min <= max` must
/// hold; [`draw_range_inclusive`] debug-asserts it.
#[derive(Debug, Clone, Copy)]
pub struct BeatConfig {
    /// The fewest beats to splice onto each stage node (must be `<= max`).
    pub beats_per_stage_min: usize,
    /// The most beats to splice onto each stage node (must be `>= min`).
    pub beats_per_stage_max: usize,
}

impl Default for BeatConfig {
    fn default() -> Self {
        Self {
            beats_per_stage_min: DEFAULT_BEATS_MIN,
            beats_per_stage_max: DEFAULT_BEATS_MAX,
        }
    }
}

/// Expands each existing narrative node into a short chain of grounded beats, growing
/// the structure to a playable length through the edit surface.
#[derive(Debug, Default, Clone, Copy)]
pub struct BeatPass {
    config: BeatConfig,
}

impl BeatPass {
    /// Construct the beat pass with the default [`BeatConfig`].
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct the beat pass with an explicit configuration.
    #[must_use]
    pub fn with_config(config: BeatConfig) -> Self {
        Self { config }
    }
}

impl ProceduralPass for BeatPass {
    fn name(&self) -> &'static str {
        "BeatPass"
    }

    fn apply(&self, world: &mut World, rng: &mut ChaCha8Rng) -> Result<(), GenError> {
        let mut original_ids: Vec<NarrativeNodeId> = world.story.structure.nodes.keys().collect();
        original_ids.sort_unstable();

        for source in original_ids {
            let Some((stage, target)) = spine_edge(world, source) else {
                continue;
            };

            let beat_count = draw_range_inclusive(
                rng,
                self.config.beats_per_stage_min,
                self.config.beats_per_stage_max,
            );
            let functions = arc_functions(stage);

            let mut previous = source;
            for beat_index in 0..beat_count {
                let ordinal = beat_index + 1;
                let hint = format!("beat {ordinal} expanding the {stage:?} stage");
                let mut spec = NodeSpec::new(format!("{stage:?}_beat_{ordinal}"), stage, hint);
                if !functions.is_empty() {
                    let function = functions[draw_range_inclusive(rng, 0, functions.len() - 1)];
                    spec.functions = BTreeSet::from([function]);
                }
                let edit = NarrativeEdit::InsertBeat {
                    source: previous,
                    target,
                    spec,
                };
                let EditOutcome::NodeAdded(new_id) = world.story.structure.apply_edit(&edit)?
                else {
                    return Err(GenError::Invariant(
                        "InsertBeat must report the new node id",
                    ));
                };
                previous = new_id;
            }
        }

        world.story.structure.recompute_kinds();
        world.story.structure.validate()?;
        Ok(())
    }
}

/// The Campbell stage of `source` and the target of its primary spine edge (the first
/// [`EdgeKind::Sequence`] out-edge), or `None` if `source` is unknown or terminal.
fn spine_edge(world: &World, source: NarrativeNodeId) -> Option<(MonomythStage, NarrativeNodeId)> {
    let node = world.story.structure.node(source)?;
    let edge = node
        .out
        .iter()
        .find(|edge| edge.kind == EdgeKind::Sequence)?;
    Some((node.stage, edge.target))
}
