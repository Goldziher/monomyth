//! [`StageReclassifyingExtractor`] — the structure-preserving [`Extractor`]
//! slice described in `monomyth-contracts`.
//!
//! ADR-0018's fuller extraction signature (deriving structure *and*
//! classification from raw source text) is future work. This implementation
//! takes a skeleton [`World`] whose narrative structure is already given and
//! re-derives only the scored `stage` axis per node, so extraction quality can
//! be measured against a gold fixture on the classification axis alone,
//! without the structure-derivation problem confounding the result.

use async_trait::async_trait;

use monomyth_contracts::{Classifier, ExtractError, Extractor, PassageRetriever};
use monomyth_core::{NarrativeEdit, World};

use crate::classifier::RagSoftmaxClassifier;

/// Re-derive every narrative node's Campbell-stage classification over a
/// skeleton [`World`], leaving every other structural field untouched.
#[derive(Debug, Clone, Copy)]
pub struct StageReclassifyingExtractor<R: PassageRetriever> {
    classifier: RagSoftmaxClassifier<R>,
}

impl<R: PassageRetriever> StageReclassifyingExtractor<R> {
    /// Build an extractor over `classifier`.
    #[must_use]
    pub const fn new(classifier: RagSoftmaxClassifier<R>) -> Self {
        Self { classifier }
    }
}

#[async_trait]
impl<R: PassageRetriever> Extractor for StageReclassifyingExtractor<R> {
    async fn extract(&self, skeleton: &World) -> Result<World, ExtractError> {
        let mut cloned = skeleton.clone();

        let mut edits = Vec::with_capacity(cloned.story.structure.nodes.len());
        for (node_id, node) in &cloned.story.structure.nodes {
            let hint = &node.synopsis.prompt().hint;
            let stage = self.classifier.classify(hint).await?;
            edits.push(NarrativeEdit::SetNodeStage {
                node: node_id,
                stage,
            });
        }

        cloned.story.structure.apply_edits(&edits)?;
        Ok(cloned)
    }
}
