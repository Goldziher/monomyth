//! Fine-tune export: turn a gold [`World`] fixture into text↔scored-structure
//! training pairs (ADR-0023 B6).
//!
//! Each spine node becomes one [`TrainingExample`] pairing the node's *authored
//! synopsis hint* (the model input) with its gold scored classification (the
//! target). Because every benchmark fixture's prose slot is `Content::Empty`
//! (ADR-0023), the `text` side carries no source prose — only the hint our own
//! procedural layer wrote — so an exported example never redistributes a source
//! passage. The licensing gate on *which* fixtures may be exported at all
//! (public-domain-tier `source_id` only) is enforced by the caller against the
//! ledger; this module is the pure, deterministic transformation.
//!
//! # Scope
//!
//! Examples are produced over the primary [`spine`](monomyth_core::NarrativeStructure::spine)
//! in fabula order, matching the node-alignment scorer's spine-only scope. Every
//! fixture that exists today is a linear chain, so the spine is the whole fixture;
//! exporting forked branches is future work.

use std::collections::BTreeMap;

use serde::Serialize;

use monomyth_core::{Weight, World};

/// One text↔scored-structure training pair for a single classification axis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TrainingExample {
    /// The model input: the node's authored synopsis hint. Never source prose —
    /// fixtures carry no prose (see the module doc).
    pub text: String,
    /// The classification axis this example teaches (e.g. `campbell_macro`).
    pub axis: String,
    /// The gold primary label (the winning stage's canonical name).
    pub primary: String,
    /// The full scored distribution, `label -> permille weight`. The primary
    /// carries [`Weight::FULL`] (1000); each scored alternative carries its own
    /// permille weight. Keyed by label name so the map is canonically ordered
    /// and snapshot-stable.
    pub distribution: BTreeMap<String, u16>,
}

/// Build one [`TrainingExample`] per spine node from `world`'s gold Campbell
/// `stage` classification, labeling every example with `axis`.
///
/// Deterministic: spine order is fabula order, and every derived collection is a
/// `BTreeMap`, so the same fixture always exports byte-identical JSONL.
#[must_use]
pub fn stage_training_examples(world: &World, axis: &str) -> Vec<TrainingExample> {
    world
        .story
        .structure
        .spine()
        .into_iter()
        .map(|node_id| {
            let node = &world.story.structure.nodes[node_id];
            let stage = &node.stage;
            let primary = stage.primary().info().name.clone();

            let mut distribution = BTreeMap::new();
            distribution.insert(primary.clone(), Weight::FULL.permille());
            for (alternative, weight) in stage.alternatives() {
                distribution.insert(alternative.info().name.clone(), weight.permille());
            }

            TrainingExample {
                text: node.synopsis.prompt().hint.clone(),
                axis: axis.to_owned(),
                primary,
                distribution,
            }
        })
        .collect()
}

/// Serialize training examples as JSON Lines: one compact JSON object per line,
/// newline-separated (no trailing newline — the caller adds one when writing a
/// file).
///
/// # Errors
///
/// Propagates any [`serde_json`] serialization error (unreachable for
/// [`TrainingExample`], whose fields are all plainly serializable, but surfaced
/// rather than panicked).
pub fn to_jsonl(examples: &[TrainingExample]) -> Result<String, serde_json::Error> {
    let mut lines = Vec::with_capacity(examples.len());
    for example in examples {
        lines.push(serde_json::to_string(example)?);
    }
    Ok(lines.join("\n"))
}
