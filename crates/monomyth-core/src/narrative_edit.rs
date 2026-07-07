//! An edit vocabulary over [`NarrativeStructure`]: a small command language for
//! mutating the narrative graph.
//!
//! A later authoring or repair layer needs to reshape the branching narrative —
//! splice a beat in, reroute a choice, retag a stage — without hand-editing the
//! [`SlotMap`](slotmap::SlotMap) and re-deriving [`NodeKind`]s by hand. Each such
//! mutation is a [`NarrativeEdit`] value: pure, serializable, and deterministic
//! (no RNG). This keeps edits inspectable and replayable, and keeps the topology
//! invariants in one place.
//!
//! Two entry points sit on [`NarrativeStructure`]:
//!
//! - [`apply_edit`](NarrativeStructure::apply_edit) is the raw building block: it
//!   applies a single op with only *local* precondition checks and neither
//!   recomputes kinds nor validates. It can leave the structure temporarily
//!   inconsistent (e.g. a freshly added node is an orphan until it is wired in).
//! - [`apply_edits`](NarrativeStructure::apply_edits) is transactional: it works on
//!   a clone, [`recompute_kinds`](NarrativeStructure::recompute_kinds), then
//!   [`validate`](NarrativeStructure::validate)s, and only overwrites the original
//!   on success — so a batch either lands whole and well-formed or not at all.

use std::collections::BTreeSet;
use std::fmt;

use monomyth_frameworks::{MonomythStage, MotifClass, PoltiSituation, ProppFunction};
use serde::{Deserialize, Serialize};

use crate::content::{Content, ContentKind, ContentPrompt};
use crate::ids::NarrativeNodeId;
use crate::narrative::{
    EdgeKind, NarrativeEdge, NarrativeError, NarrativeNode, NarrativeStructure, NodeKind,
};

/// The structural recipe for a new [`NarrativeNode`], independent of any topology.
///
/// Node creation (via [`AddNode`](NarrativeEdit::AddNode),
/// [`InsertBeat`](NarrativeEdit::InsertBeat), [`MoveBeat`](NarrativeEdit::MoveBeat))
/// needs a node's *authorial* data — label, Campbell stage, framework anchors, and
/// the grounding hint for its prose slot — but never its [`NodeKind`]: the kind is
/// a function of the graph, so it is left to
/// [`recompute_kinds`](NarrativeStructure::recompute_kinds) at commit time. Keeping
/// the spec separate from the node lets an edit describe a beat without asserting
/// anything false about its (not-yet-known) place in the graph.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeSpec {
    /// The stable structural label, e.g. `"CallToAdventure"`.
    pub label: String,
    /// The macro anchor: which Campbell stage this beat covers.
    pub stage: MonomythStage,
    /// Grounding guidance for the node's empty [`ContentKind::Synopsis`] slot.
    pub synopsis_hint: String,
    /// The Polti dramatic situation this beat instantiates, if any.
    pub situation: Option<PoltiSituation>,
    /// The realized subset of Propp functions.
    pub functions: BTreeSet<ProppFunction>,
    /// The realized Thompson motif classes.
    pub motifs: BTreeSet<MotifClass>,
}

impl NodeSpec {
    /// Build a spec with `label`, `stage`, and a `synopsis_hint`, and no realized
    /// situation, functions, or motifs.
    ///
    /// The framework subsets start empty because a fresh beat has no *realized*
    /// meso/micro content yet — those are populated by later edits or generation.
    ///
    /// ```
    /// use monomyth_core::NodeSpec;
    /// use monomyth_frameworks::MonomythStage;
    ///
    /// let spec = NodeSpec::new("CallToAdventure", MonomythStage::CallToAdventure, "the herald");
    /// assert_eq!(spec.stage, MonomythStage::CallToAdventure);
    /// assert!(spec.situation.is_none());
    /// assert!(spec.functions.is_empty());
    /// ```
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        stage: MonomythStage,
        synopsis_hint: impl Into<String>,
    ) -> Self {
        Self {
            label: label.into(),
            stage,
            synopsis_hint: synopsis_hint.into(),
            situation: None,
            functions: BTreeSet::new(),
            motifs: BTreeSet::new(),
        }
    }
}

/// One mutation of a [`NarrativeStructure`].
///
/// Each variant is a self-contained command carrying exactly the ids and data the
/// op needs. Edge-addressing variants name an edge by its `source`/`target` pair
/// (there is at most one edge per pair), mirroring [`NarrativeEdge`]'s addressing.
/// Apply a single op with [`apply_edit`](NarrativeStructure::apply_edit) or a
/// validated batch with [`apply_edits`](NarrativeStructure::apply_edits).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NarrativeEdit {
    /// Insert a new, unconnected node from `spec`. Returns its new id.
    AddNode {
        /// The recipe for the new node.
        spec: NodeSpec,
    },
    /// Remove `node`, every edge pointing at it, and its ending registration.
    RemoveNode {
        /// The node to remove.
        node: NarrativeNodeId,
    },
    /// Add an out-edge `source` → `target` of `kind` with an empty choice label.
    Connect {
        /// The edge's origin.
        source: NarrativeNodeId,
        /// The edge's destination.
        target: NarrativeNodeId,
        /// How the edge is taken.
        kind: EdgeKind,
    },
    /// Remove the out-edge `source` → `target`.
    Disconnect {
        /// The edge's origin.
        source: NarrativeNodeId,
        /// The edge's destination.
        target: NarrativeNodeId,
    },
    /// Repoint the existing edge `source` → `old_target` at `new_target`, keeping
    /// its kind, label, and guard.
    RetargetEdge {
        /// The edge's origin.
        source: NarrativeNodeId,
        /// The current destination.
        old_target: NarrativeNodeId,
        /// The new destination.
        new_target: NarrativeNodeId,
    },
    /// Set the [`EdgeKind`] of the edge `source` → `target`.
    SetEdgeKind {
        /// The edge's origin.
        source: NarrativeNodeId,
        /// The edge's destination.
        target: NarrativeNodeId,
        /// The new edge kind.
        kind: EdgeKind,
    },
    /// Set (or clear) the availability guard of the edge `source` → `target`.
    SetEdgeGuard {
        /// The edge's origin.
        source: NarrativeNodeId,
        /// The edge's destination.
        target: NarrativeNodeId,
        /// The new guard key, or `None` to clear it.
        guard: Option<String>,
    },
    /// Set `node`'s structural label.
    RelabelNode {
        /// The node to relabel.
        node: NarrativeNodeId,
        /// The new label.
        label: String,
    },
    /// Set `node`'s Campbell stage anchor.
    SetNodeStage {
        /// The node to retag.
        node: NarrativeNodeId,
        /// The new stage.
        stage: MonomythStage,
    },
    /// Set (or clear) `node`'s Polti situation.
    SetNodeSituation {
        /// The node to retag.
        node: NarrativeNodeId,
        /// The new situation, or `None` to clear it.
        situation: Option<PoltiSituation>,
    },
    /// Replace `node`'s realized Propp functions.
    SetNodeFunctions {
        /// The node to retag.
        node: NarrativeNodeId,
        /// The new function set.
        functions: BTreeSet<ProppFunction>,
    },
    /// Replace `node`'s realized Thompson motif classes.
    SetNodeMotifs {
        /// The node to retag.
        node: NarrativeNodeId,
        /// The new motif set.
        motifs: BTreeSet<MotifClass>,
    },
    /// Register `node` as an ending.
    MarkEnding {
        /// The node to register.
        node: NarrativeNodeId,
    },
    /// Deregister `node` as an ending (a no-op if it was not one).
    UnmarkEnding {
        /// The node to deregister.
        node: NarrativeNodeId,
    },
    /// Splice a new node from `spec` onto the edge `source` → `target`: the existing
    /// edge is repointed at the new node (keeping its kind, label, and guard) and a
    /// new [`EdgeKind::Sequence`] edge runs from the new node to `target`. Returns
    /// the new id.
    InsertBeat {
        /// The origin of the edge to splice onto.
        source: NarrativeNodeId,
        /// The destination of the edge to splice onto.
        target: NarrativeNodeId,
        /// The recipe for the spliced-in node.
        spec: NodeSpec,
    },
    /// Heal-and-remove a plain beat: `node` must have exactly one predecessor and
    /// one successor; the predecessor's edge is repointed at the successor and
    /// `node` is removed.
    RemoveBeat {
        /// The beat to remove.
        node: NarrativeNodeId,
    },
    /// Lift a plain beat out of its current place (healing the gap) and re-splice it
    /// onto the edge `source` → `target`. Returns the beat's new id.
    MoveBeat {
        /// The beat to move.
        node: NarrativeNodeId,
        /// The origin of the edge to re-splice onto.
        source: NarrativeNodeId,
        /// The destination of the edge to re-splice onto.
        target: NarrativeNodeId,
    },
}

/// The result of applying one [`NarrativeEdit`].
///
/// Node-creating ops report the new id so a caller can wire it up in a following
/// edit; everything else is [`Applied`](EditOutcome::Applied).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditOutcome {
    /// A node was created; carries its new id.
    NodeAdded(NarrativeNodeId),
    /// The edit mutated the structure without creating a node.
    Applied,
}

/// Why a [`NarrativeEdit`] could not be applied.
///
/// The local variants are the precondition failures an individual op checks;
/// [`Validation`](EditError::Validation) wraps the graph-invariant failure a
/// transactional [`apply_edits`](NarrativeStructure::apply_edits) surfaces after
/// recomputing kinds.
///
/// `Display` and [`std::error::Error`] are implemented by hand rather than derived
/// with `thiserror`: the edge variants carry a data field literally named `source`
/// (the edge's origin), which `thiserror` reserves for the error's own
/// [`source`](std::error::Error::source) chain and would try to treat as a nested
/// error. The manual impl keeps the plan's field names while still chaining the
/// wrapped [`NarrativeError`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    /// A referenced node does not exist.
    NodeNotFound(NarrativeNodeId),
    /// The addressed edge `source` → `target` does not exist.
    EdgeNotFound {
        /// The edge's origin.
        source: NarrativeNodeId,
        /// The edge's destination.
        target: NarrativeNodeId,
    },
    /// An edge `source` → `target` already exists where a new one was required.
    EdgeExists {
        /// The edge's origin.
        source: NarrativeNodeId,
        /// The edge's destination.
        target: NarrativeNodeId,
    },
    /// An edge would point a node at itself.
    SelfLoop(NarrativeNodeId),
    /// The root node cannot be removed as a beat.
    CannotRemoveRoot,
    /// A beat op targeted a node that is not a plain beat (in-degree or out-degree
    /// other than one).
    NotABeat(NarrativeNodeId),
    /// Healing a removed beat would collide with an existing `predecessor` →
    /// `successor` edge.
    HealCollision {
        /// The predecessor whose edge is being repointed.
        predecessor: NarrativeNodeId,
        /// The successor the edge would point at.
        successor: NarrativeNodeId,
    },
    /// The batch produced a structure that fails a graph invariant.
    Validation(NarrativeError),
}

impl fmt::Display for EditError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NodeNotFound(node) => write!(formatter, "node {node:?} does not exist"),
            Self::EdgeNotFound { source, target } => {
                write!(formatter, "no edge from {source:?} to {target:?}")
            }
            Self::EdgeExists { source, target } => {
                write!(
                    formatter,
                    "an edge from {source:?} to {target:?} already exists"
                )
            }
            Self::SelfLoop(node) => {
                write!(formatter, "node {node:?} cannot have an edge to itself")
            }
            Self::CannotRemoveRoot => {
                write!(formatter, "the root node cannot be removed as a beat")
            }
            Self::NotABeat(node) => write!(
                formatter,
                "node {node:?} is not a plain beat (needs exactly one in-edge and one out-edge)"
            ),
            Self::HealCollision {
                predecessor,
                successor,
            } => write!(
                formatter,
                "healing would collide with an existing edge from {predecessor:?} to {successor:?}"
            ),
            Self::Validation(error) => {
                write!(formatter, "the edited structure is invalid: {error}")
            }
        }
    }
}

impl std::error::Error for EditError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Validation(error) => Some(error),
            _ => None,
        }
    }
}

impl From<NarrativeError> for EditError {
    fn from(error: NarrativeError) -> Self {
        Self::Validation(error)
    }
}

/// An empty choice-prose slot, the seam every new edge label carries.
fn empty_choice_label() -> Content {
    Content::empty(ContentPrompt::new(ContentKind::Choice, ""))
}

/// Build a [`NarrativeNode`] from a [`NodeSpec`], with a placeholder [`NodeKind`]
/// that [`recompute_kinds`](NarrativeStructure::recompute_kinds) corrects at commit.
fn node_from_spec(spec: &NodeSpec) -> NarrativeNode {
    let synopsis = Content::empty(ContentPrompt::new(
        ContentKind::Synopsis,
        spec.synopsis_hint.clone(),
    ));
    let mut node = NarrativeNode::new(spec.label.clone(), NodeKind::Beat, spec.stage, synopsis);
    node.situation = spec.situation;
    node.functions.clone_from(&spec.functions);
    node.motifs.clone_from(&spec.motifs);
    node
}

impl NarrativeStructure {
    /// Apply a single [`NarrativeEdit`] with only local precondition checks.
    ///
    /// This is the raw building block: it does **not**
    /// [`recompute_kinds`](Self::recompute_kinds) or [`validate`](Self::validate),
    /// so it can leave the structure temporarily inconsistent (a fresh node is an
    /// orphan until wired in). Prefer [`apply_edits`](Self::apply_edits) when you
    /// need the batch to end well-formed.
    ///
    /// # Errors
    ///
    /// Returns the [`EditError`] naming the local precondition the op violated
    /// (missing node/edge, duplicate edge, self-loop, or a beat-shape violation).
    ///
    /// ```
    /// use monomyth_core::{EdgeKind, EditOutcome, NarrativeEdit, NodeSpec};
    /// use monomyth_frameworks::MonomythStage;
    /// # use monomyth_core::doc_support::single_node_structure;
    ///
    /// let mut structure = single_node_structure();
    /// let root = structure.root();
    /// let EditOutcome::NodeAdded(beat) = structure.apply_edit(&NarrativeEdit::AddNode {
    ///     spec: NodeSpec::new("Return", MonomythStage::FreedomToLive, "the return"),
    /// })? else { unreachable!() };
    /// structure.apply_edit(&NarrativeEdit::Connect {
    ///     source: root,
    ///     target: beat,
    ///     kind: EdgeKind::Sequence,
    /// })?;
    /// assert_eq!(structure.children(root).count(), 1);
    /// # Ok::<(), monomyth_core::EditError>(())
    /// ```
    pub fn apply_edit(&mut self, edit: &NarrativeEdit) -> Result<EditOutcome, EditError> {
        match edit {
            NarrativeEdit::AddNode { spec } => {
                let id = self.nodes.insert(node_from_spec(spec));
                Ok(EditOutcome::NodeAdded(id))
            }
            NarrativeEdit::RemoveNode { node } => {
                self.remove_node(*node)?;
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::Connect {
                source,
                target,
                kind,
            } => {
                self.connect(*source, *target, *kind)?;
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::Disconnect { source, target } => {
                self.disconnect(*source, *target)?;
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::RetargetEdge {
                source,
                old_target,
                new_target,
            } => {
                self.retarget_edge(*source, *old_target, *new_target)?;
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::SetEdgeKind {
                source,
                target,
                kind,
            } => {
                self.edge_mut(*source, *target)?.kind = *kind;
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::SetEdgeGuard {
                source,
                target,
                guard,
            } => {
                self.edge_mut(*source, *target)?.guard.clone_from(guard);
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::RelabelNode { node, label } => {
                self.node_mut(*node)?.label.clone_from(label);
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::SetNodeStage { node, stage } => {
                self.node_mut(*node)?.stage = *stage;
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::SetNodeSituation { node, situation } => {
                self.node_mut(*node)?.situation = *situation;
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::SetNodeFunctions { node, functions } => {
                self.node_mut(*node)?.functions.clone_from(functions);
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::SetNodeMotifs { node, motifs } => {
                self.node_mut(*node)?.motifs.clone_from(motifs);
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::MarkEnding { node } => {
                self.mark_ending(*node)?;
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::UnmarkEnding { node } => {
                self.endings.remove(node);
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::InsertBeat {
                source,
                target,
                spec,
            } => {
                let id = self.insert_beat(*source, *target, spec)?;
                Ok(EditOutcome::NodeAdded(id))
            }
            NarrativeEdit::RemoveBeat { node } => {
                self.remove_beat(*node)?;
                Ok(EditOutcome::Applied)
            }
            NarrativeEdit::MoveBeat {
                node,
                source,
                target,
            } => {
                let id = self.move_beat(*node, *source, *target)?;
                Ok(EditOutcome::NodeAdded(id))
            }
        }
    }

    /// Apply a batch of edits transactionally.
    ///
    /// Works on a clone: each edit is applied in turn, then
    /// [`recompute_kinds`](Self::recompute_kinds) and [`validate`](Self::validate)
    /// run once. On any error — a local precondition or the final validation — the
    /// original is left untouched and the error is returned; on success the original
    /// is overwritten and the per-edit [`EditOutcome`]s are returned in order.
    ///
    /// # Errors
    ///
    /// Returns the first [`EditError`] a member op raised, or
    /// [`Validation`](EditError::Validation) if the batch left the graph malformed.
    ///
    /// ```
    /// use monomyth_core::{EdgeKind, EditOutcome, NarrativeEdit, NodeSpec};
    /// use monomyth_frameworks::MonomythStage;
    /// # use monomyth_core::doc_support::single_node_structure;
    ///
    /// let mut structure = single_node_structure();
    /// let root = structure.root();
    /// // Add a beat, then wire it in and move the ending onto it — atomically.
    /// let EditOutcome::NodeAdded(beat) = structure.apply_edit(&NarrativeEdit::AddNode {
    ///     spec: NodeSpec::new("Return", MonomythStage::FreedomToLive, "the return"),
    /// })? else { unreachable!() };
    /// let outcomes = structure.apply_edits(&[
    ///     NarrativeEdit::Connect { source: root, target: beat, kind: EdgeKind::Sequence },
    ///     NarrativeEdit::MarkEnding { node: beat },
    ///     NarrativeEdit::UnmarkEnding { node: root },
    /// ])?;
    /// assert_eq!(outcomes.len(), 3);
    /// structure.validate()?;
    /// # Ok::<(), monomyth_core::EditError>(())
    /// ```
    pub fn apply_edits(&mut self, edits: &[NarrativeEdit]) -> Result<Vec<EditOutcome>, EditError> {
        let mut working = self.clone();
        let mut outcomes = Vec::with_capacity(edits.len());
        for edit in edits {
            outcomes.push(working.apply_edit(edit)?);
        }
        working.recompute_kinds();
        working.validate()?;
        *self = working;
        Ok(outcomes)
    }

    /// The index of the out-edge `source` → `target`, if the pair is connected.
    fn out_edge_index(&self, source: NarrativeNodeId, target: NarrativeNodeId) -> Option<usize> {
        self.nodes
            .get(source)?
            .out
            .iter()
            .position(|edge| edge.target == target)
    }

    /// Every edge that points at `node`, as `(predecessor id, index into its `out`)`.
    ///
    /// Shared by the removal ops, which must know a node's predecessors to heal or
    /// prune the edges into it.
    fn in_edges_of(&self, node: NarrativeNodeId) -> Vec<(NarrativeNodeId, usize)> {
        let mut predecessors = Vec::new();
        for (id, source) in &self.nodes {
            for (index, edge) in source.out.iter().enumerate() {
                if edge.target == node {
                    predecessors.push((id, index));
                }
            }
        }
        predecessors
    }

    /// A mutable reference to `node`, or [`NodeNotFound`](EditError::NodeNotFound).
    fn node_mut(&mut self, node: NarrativeNodeId) -> Result<&mut NarrativeNode, EditError> {
        self.nodes
            .get_mut(node)
            .ok_or(EditError::NodeNotFound(node))
    }

    /// A mutable reference to the edge `source` → `target`, or
    /// [`EdgeNotFound`](EditError::EdgeNotFound).
    fn edge_mut(
        &mut self,
        source: NarrativeNodeId,
        target: NarrativeNodeId,
    ) -> Result<&mut NarrativeEdge, EditError> {
        let index = self
            .out_edge_index(source, target)
            .ok_or(EditError::EdgeNotFound { source, target })?;
        Ok(&mut self.nodes[source].out[index])
    }

    /// Remove `node`, every edge into it, and its ending registration.
    fn remove_node(&mut self, node: NarrativeNodeId) -> Result<(), EditError> {
        if !self.nodes.contains_key(node) {
            return Err(EditError::NodeNotFound(node));
        }
        for source in self.nodes.values_mut() {
            source.out.retain(|edge| edge.target != node);
        }
        self.nodes.remove(node);
        self.endings.remove(&node);
        Ok(())
    }

    /// Add an out-edge `source` → `target` of `kind` with an empty choice label.
    fn connect(
        &mut self,
        source: NarrativeNodeId,
        target: NarrativeNodeId,
        kind: EdgeKind,
    ) -> Result<(), EditError> {
        if source == target {
            return Err(EditError::SelfLoop(source));
        }
        if !self.nodes.contains_key(target) {
            return Err(EditError::NodeNotFound(target));
        }
        if self.out_edge_index(source, target).is_some() {
            return Err(EditError::EdgeExists { source, target });
        }
        let edge = NarrativeEdge::new(target, kind, empty_choice_label());
        self.node_mut(source)?.out.push(edge);
        Ok(())
    }

    /// Remove the out-edge `source` → `target`.
    fn disconnect(
        &mut self,
        source: NarrativeNodeId,
        target: NarrativeNodeId,
    ) -> Result<(), EditError> {
        let index = self
            .out_edge_index(source, target)
            .ok_or(EditError::EdgeNotFound { source, target })?;
        self.nodes[source].out.remove(index);
        Ok(())
    }

    /// Register `node` as an ending, checking it exists first.
    fn mark_ending(&mut self, node: NarrativeNodeId) -> Result<(), EditError> {
        if !self.nodes.contains_key(node) {
            return Err(EditError::NodeNotFound(node));
        }
        self.endings.insert(node);
        Ok(())
    }

    /// Repoint the edge `source` → `old_target` at `new_target`, keeping kind/label/guard.
    fn retarget_edge(
        &mut self,
        source: NarrativeNodeId,
        old_target: NarrativeNodeId,
        new_target: NarrativeNodeId,
    ) -> Result<(), EditError> {
        if new_target == source {
            return Err(EditError::SelfLoop(source));
        }
        let index = self
            .out_edge_index(source, old_target)
            .ok_or(EditError::EdgeNotFound {
                source,
                target: old_target,
            })?;
        if self.out_edge_index(source, new_target).is_some() {
            return Err(EditError::EdgeExists {
                source,
                target: new_target,
            });
        }
        if !self.nodes.contains_key(new_target) {
            return Err(EditError::NodeNotFound(new_target));
        }
        self.nodes[source].out[index].target = new_target;
        Ok(())
    }

    /// Splice a new node from `spec` onto the edge `source` → `target`.
    fn insert_beat(
        &mut self,
        source: NarrativeNodeId,
        target: NarrativeNodeId,
        spec: &NodeSpec,
    ) -> Result<NarrativeNodeId, EditError> {
        let index = self
            .out_edge_index(source, target)
            .ok_or(EditError::EdgeNotFound { source, target })?;
        let new_id = self.nodes.insert(node_from_spec(spec));
        // Redirect the existing edge to the new node, preserving its kind/label/guard.
        self.nodes[source].out[index].target = new_id;
        // Run a fresh Sequence edge from the new node to the original target.
        self.nodes[new_id].out.push(NarrativeEdge::new(
            target,
            EdgeKind::Sequence,
            empty_choice_label(),
        ));
        Ok(new_id)
    }

    /// Heal-and-remove a plain beat with exactly one predecessor and one successor.
    fn remove_beat(&mut self, node: NarrativeNodeId) -> Result<(), EditError> {
        if node == self.root {
            return Err(EditError::CannotRemoveRoot);
        }
        let predecessors = self.in_edges_of(node);
        let Some(beat) = self.nodes.get(node) else {
            return Err(EditError::NotABeat(node));
        };
        if predecessors.len() != 1 || beat.out.len() != 1 {
            return Err(EditError::NotABeat(node));
        }
        let successor = beat.out[0].target;
        let (predecessor, predecessor_index) = predecessors[0];
        if self.out_edge_index(predecessor, successor).is_some() {
            return Err(EditError::HealCollision {
                predecessor,
                successor,
            });
        }
        // Repoint the predecessor's edge at the successor, preserving kind/label/guard.
        self.nodes[predecessor].out[predecessor_index].target = successor;
        self.nodes.remove(node);
        self.endings.remove(&node);
        Ok(())
    }

    /// Lift `node` out of its place (healing the gap) and re-splice it onto
    /// `source` → `target`.
    fn move_beat(
        &mut self,
        node: NarrativeNodeId,
        source: NarrativeNodeId,
        target: NarrativeNodeId,
    ) -> Result<NarrativeNodeId, EditError> {
        let spec = {
            let Some(beat) = self.nodes.get(node) else {
                return Err(EditError::NotABeat(node));
            };
            NodeSpec {
                label: beat.label.clone(),
                stage: beat.stage,
                synopsis_hint: beat.synopsis.prompt().hint.clone(),
                situation: beat.situation,
                functions: beat.functions.clone(),
                motifs: beat.motifs.clone(),
            }
        };
        self.remove_beat(node)?;
        self.insert_beat(source, target, &spec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> NarrativeStructure {
        crate::doc_support::single_node_structure()
    }

    #[test]
    fn should_commit_and_recompute_kinds_on_valid_batch() {
        let mut structure = base();
        let root = structure.root();
        let EditOutcome::NodeAdded(beat) = structure
            .apply_edit(&NarrativeEdit::AddNode {
                spec: NodeSpec::new("Return", MonomythStage::FreedomToLive, "the return"),
            })
            .unwrap()
        else {
            panic!("AddNode must report the new id");
        };

        let outcomes = structure
            .apply_edits(&[
                NarrativeEdit::Connect {
                    source: root,
                    target: beat,
                    kind: EdgeKind::Sequence,
                },
                NarrativeEdit::MarkEnding { node: beat },
                NarrativeEdit::UnmarkEnding { node: root },
            ])
            .unwrap();

        assert_eq!(
            outcomes,
            vec![
                EditOutcome::Applied,
                EditOutcome::Applied,
                EditOutcome::Applied
            ]
        );
        assert_eq!(structure.node(root).unwrap().kind, NodeKind::Origin);
        assert_eq!(structure.node(beat).unwrap().kind, NodeKind::Ending);
        structure.validate().unwrap();
    }

    #[test]
    fn should_roll_back_when_batch_fails_validation() {
        let mut structure = base();
        let node_count_before = structure.nodes.len();
        let root_before = structure.root();

        // An unconnected AddNode leaves a second source, so validation must reject it.
        let result = structure.apply_edits(&[NarrativeEdit::AddNode {
            spec: NodeSpec::new("Orphan", MonomythStage::FreedomToLive, "an orphan beat"),
        }]);

        assert!(matches!(result, Err(EditError::Validation(_))));
        assert_eq!(
            structure.nodes.len(),
            node_count_before,
            "rollback must not add the node"
        );
        assert_eq!(structure.root(), root_before);
    }

    #[test]
    fn should_reject_self_loop_connect() {
        let mut structure = base();
        let root = structure.root();
        let error = structure
            .apply_edit(&NarrativeEdit::Connect {
                source: root,
                target: root,
                kind: EdgeKind::Sequence,
            })
            .unwrap_err();
        assert_eq!(error, EditError::SelfLoop(root));
    }
}
