//! The branching narrative structure: a single-source, acyclic, reconverging DAG
//! of story beats.
//!
//! [`NarrativeStructure`] is the story spine as a *graph*, not a flat list: player
//! choices fork the trunk and later reconverge, so two nodes can point at the same
//! target (in-degree > 1). Forward traversal (root → ending) is the dominant
//! access pattern for both the future player loop and a renderer, so edges live on
//! the node ([`NarrativeNode::out`]) rather than in a separate edge list, and each
//! choice is addressed by its stable [`NarrativeNodeId`] target.
//!
//! The vocabulary is grounded in the framework artifacts via `monomyth-frameworks`:
//! every node carries a Campbell [`MonomythStage`] anchor, and its realized Propp
//! [`ProppFunction`]s, Polti [`PoltiSituation`], and Thompson [`MotifClass`]es are
//! the meso/micro layers a later generation pass fills in.
//!
//! Like [`World`](crate::World) and [`Story`](crate::Story), [`NarrativeStructure`]
//! holds a [`SlotMap`] and so does not implement [`PartialEq`]; compare two
//! structures via their serialized form.

use std::collections::{BTreeMap, BTreeSet};

use monomyth_frameworks::{
    MonomythStage, MotifClass, PoltiSituation, ProppFunction, arc_functions,
};
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;
use thiserror::Error;

use crate::content::Content;
use crate::ids::NarrativeNodeId;

/// The validated authorial role of a [`NarrativeNode`] within the graph topology.
///
/// The kind is a declaration that [`NarrativeStructure::validate`] checks against
/// the node's actual in/out degree, so a malformed graph is rejected rather than
/// silently rendered wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum NodeKind {
    /// The single source of the graph: in-degree 0.
    Origin,
    /// A plain beat on the trunk: at most one in-edge, exactly one out-edge.
    Beat,
    /// A fork: out-degree greater than one (a player choice point).
    Branch,
    /// A join where branches reconverge: in-degree greater than one.
    Merge,
    /// A terminal beat: out-degree 0.
    Ending,
}

/// How one narrative beat leads to the next.
///
/// In a text adventure a player-facing fork is a [`Choice`](EdgeKind::Choice) edge;
/// [`Sequence`](EdgeKind::Sequence) is the linear default that also defines the
/// primary spine. [`Fork`](EdgeKind::Fork) is reserved for automatic (non-player)
/// branching a later layer may introduce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    /// The linear next beat; the first such edge defines a node's spine successor.
    Sequence,
    /// A player-selectable branch.
    Choice,
    /// An automatic, non-player branch (reserved for a later layer).
    Fork,
}

/// A directed edge from one beat to another: its target, how it is taken, its
/// choice-prose slot, and an optional gameplay guard.
///
/// Edges carry no id of their own: a choice is addressed by its stable
/// [`target`](Self::target), and there is at most one edge per `(source, target)`.
/// The empty [`label`](Self::label) and [`None`] [`guard`](Self::guard) are the
/// forward seams for the prose and interactive milestones; the v1 generator leaves
/// them empty.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NarrativeEdge {
    /// The node this edge leads to.
    pub target: NarrativeNodeId,
    /// How the edge is taken.
    pub kind: EdgeKind,
    /// The choice-prose slot (filled later; [`ContentKind::Choice`](crate::ContentKind::Choice)).
    pub label: Content,
    /// A [`WorldState::flags`](crate::WorldState) key gating availability (set later).
    pub guard: Option<String>,
}

impl NarrativeEdge {
    /// Build an edge to `target` of `kind` with its `label` slot and no guard.
    ///
    /// ```
    /// use monomyth_core::{Content, ContentKind, ContentPrompt, EdgeKind, NarrativeEdge, NarrativeNodeId};
    ///
    /// let label = Content::empty(ContentPrompt::new(ContentKind::Choice, "heed the call"));
    /// let edge = NarrativeEdge::new(NarrativeNodeId::default(), EdgeKind::Sequence, label);
    /// assert_eq!(edge.kind, EdgeKind::Sequence);
    /// assert!(edge.guard.is_none());
    /// ```
    #[must_use]
    pub fn new(target: NarrativeNodeId, kind: EdgeKind, label: Content) -> Self {
        Self {
            target,
            kind,
            label,
            guard: None,
        }
    }
}

/// A single beat in the narrative graph: its label, topological role, framework
/// anchors, prose slot, and outgoing branches.
///
/// The Campbell [`stage`](Self::stage) is the macro anchor; the Propp functions
/// that realize the stage stay *derived* (see [`arc_functions`](Self::arc_functions))
/// rather than stored. [`functions`](Self::functions), [`situation`](Self::situation),
/// and [`motifs`](Self::motifs) are the realized meso/micro subset a later layer
/// sets — empty in v1.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NarrativeNode {
    /// A stable structural label, e.g. `"CallToAdventure"`.
    pub label: String,
    /// The validated authorial role of this node (see [`NodeKind`]).
    pub kind: NodeKind,
    /// The macro anchor: which Campbell stage this beat covers.
    pub stage: MonomythStage,
    /// The realized subset of Propp functions (meso; set later, empty in v1).
    pub functions: BTreeSet<ProppFunction>,
    /// The Polti dramatic situation this beat instantiates, if any (meso; set later).
    pub situation: Option<PoltiSituation>,
    /// The realized Thompson motif classes (micro; set later, empty in v1).
    pub motifs: BTreeSet<MotifClass>,
    /// The empty prose slot a future content layer fills.
    pub synopsis: Content,
    /// The outgoing branches; index 0 is the primary/spine edge.
    pub out: Vec<NarrativeEdge>,
}

impl NarrativeNode {
    /// Build a node with `label`, `kind`, `stage`, and its `synopsis` slot, and no
    /// realized functions, situation, motifs, or out-edges.
    ///
    /// ```
    /// use monomyth_core::{Content, ContentKind, ContentPrompt, NarrativeNode, NodeKind};
    /// use monomyth_frameworks::MonomythStage;
    ///
    /// let synopsis = Content::empty(ContentPrompt::new(ContentKind::Synopsis, "the call"));
    /// let node = NarrativeNode::new("CallToAdventure", NodeKind::Origin, MonomythStage::CallToAdventure, synopsis);
    /// assert!(node.out.is_empty());
    /// ```
    #[must_use]
    pub fn new(
        label: impl Into<String>,
        kind: NodeKind,
        stage: MonomythStage,
        synopsis: Content,
    ) -> Self {
        Self {
            label: label.into(),
            kind,
            stage,
            functions: BTreeSet::new(),
            situation: None,
            motifs: BTreeSet::new(),
            synopsis,
            out: Vec::new(),
        }
    }

    /// The Propp functions that realize this node's Campbell stage.
    ///
    /// Computed from [`arc_functions`](monomyth_frameworks::arc_functions) rather
    /// than stored, so the crosswalk artifacts remain the single source of truth:
    /// the derivation can never drift from the scholarship and is absent from the
    /// serialized world. Distinct from [`functions`](Self::functions), which is the
    /// (later-populated) *realized* subset.
    ///
    /// ```
    /// use monomyth_core::{Content, ContentKind, ContentPrompt, NarrativeNode, NodeKind};
    /// use monomyth_frameworks::{arc_functions, MonomythStage};
    ///
    /// let stage = MonomythStage::TheRoadOfTrials;
    /// let synopsis = Content::empty(ContentPrompt::new(ContentKind::Synopsis, "trials"));
    /// let node = NarrativeNode::new("TheRoadOfTrials", NodeKind::Beat, stage, synopsis);
    /// assert_eq!(node.arc_functions(), arc_functions(stage));
    /// ```
    #[must_use]
    pub fn arc_functions(&self) -> &'static [ProppFunction] {
        arc_functions(self.stage)
    }
}

/// A single-source, acyclic, reconverging DAG of story beats: the branching
/// narrative skeleton.
///
/// Holds a [`SlotMap`], so — like [`World`](crate::World) and
/// [`Story`](crate::Story) — it derives everything but [`PartialEq`]; compare two
/// structures via their serialized form. Build one and check it with
/// [`validate`](Self::validate) before relying on the query helpers.
///
/// ```
/// use std::collections::BTreeSet;
///
/// use monomyth_core::{
///     Content, ContentKind, ContentPrompt, EdgeKind, NarrativeEdge, NarrativeNode,
///     NarrativeStructure, NodeKind,
/// };
/// use monomyth_frameworks::MonomythStage;
/// use slotmap::SlotMap;
///
/// fn synopsis(hint: &str) -> Content {
///     Content::empty(ContentPrompt::new(ContentKind::Synopsis, hint))
/// }
///
/// let mut nodes = SlotMap::with_key();
/// let end = nodes.insert(NarrativeNode::new(
///     "FreedomToLive", NodeKind::Ending, MonomythStage::FreedomToLive, synopsis("the return"),
/// ));
/// let mut origin = NarrativeNode::new(
///     "CallToAdventure", NodeKind::Origin, MonomythStage::CallToAdventure, synopsis("the call"),
/// );
/// let label = Content::empty(ContentPrompt::new(ContentKind::Choice, ""));
/// origin.out.push(NarrativeEdge::new(end, EdgeKind::Sequence, label));
/// let root = nodes.insert(origin);
///
/// let structure = NarrativeStructure { nodes, root, endings: BTreeSet::from([end]) };
/// structure.validate()?;
/// assert_eq!(structure.spine(), vec![root, end]);
/// # Ok::<(), monomyth_core::NarrativeError>(())
/// ```
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct NarrativeStructure {
    /// All beats, keyed by [`NarrativeNodeId`].
    pub nodes: SlotMap<NarrativeNodeId, NarrativeNode>,
    /// The single source of the graph (in-degree 0).
    pub root: NarrativeNodeId,
    /// The terminal beats (out-degree 0); non-empty in a valid structure.
    pub endings: BTreeSet<NarrativeNodeId>,
}

/// Why a [`NarrativeStructure`] is not a well-formed branching narrative.
///
/// Node-related variants carry the offending [`NarrativeNodeId`];
/// [`validate`](NarrativeStructure::validate) iterates node keys in sorted order so
/// the smallest offending id is reported first, keeping failures snapshot-stable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Error)]
pub enum NarrativeError {
    /// The [`root`](NarrativeStructure::root) key is not present in the node map.
    #[error("root node {0:?} is missing from the structure")]
    MissingRoot(NarrativeNodeId),
    /// An edge points at a target that is not a node.
    #[error("node {node:?} has an edge to missing target {target:?}")]
    DanglingEdge {
        /// The node holding the offending edge.
        node: NarrativeNodeId,
        /// The non-existent target the edge points at.
        target: NarrativeNodeId,
    },
    /// A node has more than one edge to the same target.
    #[error("node {node:?} has duplicate edges to {target:?}")]
    DuplicateEdge {
        /// The node holding the duplicated edges.
        node: NarrativeNodeId,
        /// The target both edges point at.
        target: NarrativeNodeId,
    },
    /// A non-root node has in-degree 0, so the graph has more than one source.
    #[error("node {0:?} is a second source (in-degree 0 but not the root)")]
    OrphanSource(NarrativeNodeId),
    /// A node is not reachable from the root.
    ///
    /// A defensive guard: in an acyclic graph an unreachable node's component
    /// always contains an in-degree-0 [`OrphanSource`](Self::OrphanSource), which is
    /// reported first, so this variant does not fire for inputs that pass the
    /// earlier checks.
    #[error("node {0:?} is not reachable from the root")]
    Unreachable(NarrativeNodeId),
    /// A node participates in a cycle; the graph must be acyclic.
    #[error("node {0:?} participates in a cycle")]
    Cycle(NarrativeNodeId),
    /// A node cannot reach any ending.
    ///
    /// A defensive guard: once every out-degree-0 node is a registered ending (see
    /// [`UnregisteredEnding`](Self::UnregisteredEnding)) and the graph is acyclic,
    /// every node reaches an ending, so this variant does not fire for inputs that
    /// pass the earlier checks.
    #[error("node {0:?} cannot reach any ending")]
    DeadEnd(NarrativeNodeId),
    /// An out-degree-0 node is not registered in [`endings`](NarrativeStructure::endings).
    #[error("terminal node {0:?} is not registered as an ending")]
    UnregisteredEnding(NarrativeNodeId),
    /// A registered ending is not an out-degree-0 node (has successors or is absent).
    #[error("registered ending {0:?} is not a terminal node")]
    NonTerminalEnding(NarrativeNodeId),
    /// The [`endings`](NarrativeStructure::endings) set is empty.
    #[error("the structure has no endings")]
    NoEndings,
    /// A node's declared [`NodeKind`] disagrees with its in/out degree.
    #[error("node {0:?} has a kind that disagrees with its topology")]
    KindMismatch(NarrativeNodeId),
}

/// The traversal colour of a node during acyclicity checking.
enum Colour {
    /// On the current DFS stack.
    Grey,
    /// Fully explored.
    Black,
}

impl NarrativeStructure {
    /// The root node id (the single source).
    #[must_use]
    pub fn root(&self) -> NarrativeNodeId {
        self.root
    }

    /// The node for `id`, or `None` if no such node exists.
    #[must_use]
    pub fn node(&self, id: NarrativeNodeId) -> Option<&NarrativeNode> {
        self.nodes.get(id)
    }

    /// The targets of `id`'s out-edges, in edge order (empty if `id` is unknown).
    pub fn children(&self, id: NarrativeNodeId) -> impl Iterator<Item = NarrativeNodeId> + '_ {
        self.nodes
            .get(id)
            .into_iter()
            .flat_map(|node| node.out.iter().map(|edge| edge.target))
    }

    /// Whether `id` is a fork (out-degree greater than one).
    #[must_use]
    pub fn is_fork(&self, id: NarrativeNodeId) -> bool {
        self.nodes.get(id).is_some_and(|node| node.out.len() > 1)
    }

    /// Whether `id` is a merge (in-degree greater than one).
    #[must_use]
    pub fn is_merge(&self, id: NarrativeNodeId) -> bool {
        self.nodes
            .values()
            .flat_map(|node| node.out.iter())
            .filter(|edge| edge.target == id)
            .take(2)
            .count()
            > 1
    }

    /// Whether `id` is a registered ending.
    #[must_use]
    pub fn is_ending(&self, id: NarrativeNodeId) -> bool {
        self.endings.contains(&id)
    }

    /// The registered endings, in id order.
    pub fn endings(&self) -> impl Iterator<Item = NarrativeNodeId> + '_ {
        self.endings.iter().copied()
    }

    /// The primary spine from the root to an ending, following each node's primary
    /// out-edge — the first [`EdgeKind::Sequence`], else the first edge.
    ///
    /// The linear analogue of the old flat 17-stage arc. Terminates at the first
    /// node with no out-edge or, defensively, on revisiting a node.
    #[must_use]
    pub fn spine(&self) -> Vec<NarrativeNodeId> {
        let mut path = Vec::new();
        let mut visited = BTreeSet::new();
        let mut current = self.root;
        while visited.insert(current) {
            path.push(current);
            let Some(node) = self.nodes.get(current) else {
                break;
            };
            let next = node
                .out
                .iter()
                .find(|edge| edge.kind == EdgeKind::Sequence)
                .or_else(|| node.out.first());
            match next {
                Some(edge) => current = edge.target,
                None => break,
            }
        }
        path
    }

    /// A topological ordering of the nodes via Kahn's algorithm.
    ///
    /// Uses a [`BTreeSet`] ready-queue so the order is stable (the smallest
    /// available id is emitted next). Assumes a validated, acyclic structure; a
    /// structure with a cycle yields a partial order omitting the cyclic nodes.
    #[must_use]
    pub fn topological_order(&self) -> Vec<NarrativeNodeId> {
        let mut in_degree: BTreeMap<NarrativeNodeId, usize> =
            self.nodes.keys().map(|key| (key, 0usize)).collect();
        for node in self.nodes.values() {
            for edge in &node.out {
                if let Some(degree) = in_degree.get_mut(&edge.target) {
                    *degree += 1;
                }
            }
        }

        let mut ready: BTreeSet<NarrativeNodeId> = in_degree
            .iter()
            .filter(|&(_, &degree)| degree == 0)
            .map(|(&key, _)| key)
            .collect();

        let mut order = Vec::with_capacity(self.nodes.len());
        while let Some(&next) = ready.iter().next() {
            ready.remove(&next);
            order.push(next);
            if let Some(node) = self.nodes.get(next) {
                for edge in &node.out {
                    if let Some(degree) = in_degree.get_mut(&edge.target) {
                        *degree -= 1;
                        if *degree == 0 {
                            ready.insert(edge.target);
                        }
                    }
                }
            }
        }
        order
    }

    /// Node keys in ascending order, for snapshot-stable iteration.
    fn sorted_keys(&self) -> Vec<NarrativeNodeId> {
        let mut keys: Vec<NarrativeNodeId> = self.nodes.keys().collect();
        keys.sort_unstable();
        keys
    }

    /// Check every invariant of a well-formed branching narrative.
    ///
    /// Enforces, in order: the root exists; every edge target exists; no duplicate
    /// `(source, target)` edge; the root is the only in-degree-0 node; the graph is
    /// acyclic; every node is reachable from the root; every node can reach an
    /// ending; the endings set is non-empty and exactly the out-degree-0 set; and
    /// each node's [`NodeKind`] agrees with its topology. Node keys are iterated in
    /// sorted order, so the smallest offending id is reported first.
    ///
    /// # Errors
    ///
    /// Returns the [`NarrativeError`] naming the first invariant violated and the
    /// smallest offending [`NarrativeNodeId`].
    pub fn validate(&self) -> Result<(), NarrativeError> {
        if !self.nodes.contains_key(self.root) {
            return Err(NarrativeError::MissingRoot(self.root));
        }

        let keys = self.sorted_keys();
        let in_edges = self.build_in_edges(&keys)?;

        // Single source: only the root may have in-degree 0.
        for &key in &keys {
            let in_degree = in_edges.get(&key).map_or(0, Vec::len);
            if in_degree == 0 && key != self.root {
                return Err(NarrativeError::OrphanSource(key));
            }
        }

        self.check_acyclic(&keys)?;
        self.check_reachable(&keys)?;

        if self.endings.is_empty() {
            return Err(NarrativeError::NoEndings);
        }
        self.check_endings(&keys)?;
        self.check_reaches_ending(&keys, &in_edges)?;
        self.check_kinds(&keys, &in_edges)?;
        Ok(())
    }

    /// Build the predecessor index while checking edge targets exist and are unique.
    fn build_in_edges(
        &self,
        keys: &[NarrativeNodeId],
    ) -> Result<BTreeMap<NarrativeNodeId, Vec<NarrativeNodeId>>, NarrativeError> {
        let mut in_edges: BTreeMap<NarrativeNodeId, Vec<NarrativeNodeId>> = BTreeMap::new();
        for &source in keys {
            let mut seen: BTreeSet<NarrativeNodeId> = BTreeSet::new();
            for edge in &self.nodes[source].out {
                if !self.nodes.contains_key(edge.target) {
                    return Err(NarrativeError::DanglingEdge {
                        node: source,
                        target: edge.target,
                    });
                }
                if !seen.insert(edge.target) {
                    return Err(NarrativeError::DuplicateEdge {
                        node: source,
                        target: edge.target,
                    });
                }
                in_edges.entry(edge.target).or_default().push(source);
            }
        }
        Ok(in_edges)
    }

    /// Reject any cycle via depth-first colouring over all nodes.
    fn check_acyclic(&self, keys: &[NarrativeNodeId]) -> Result<(), NarrativeError> {
        let mut colour: BTreeMap<NarrativeNodeId, Colour> = BTreeMap::new();
        for &key in keys {
            if !colour.contains_key(&key) {
                self.dfs_acyclic(key, &mut colour)?;
            }
        }
        Ok(())
    }

    /// Depth-first visit reporting the grey node closing a cycle.
    fn dfs_acyclic(
        &self,
        node: NarrativeNodeId,
        colour: &mut BTreeMap<NarrativeNodeId, Colour>,
    ) -> Result<(), NarrativeError> {
        colour.insert(node, Colour::Grey);
        for edge in &self.nodes[node].out {
            match colour.get(&edge.target) {
                Some(Colour::Grey) => return Err(NarrativeError::Cycle(edge.target)),
                Some(Colour::Black) => {}
                None => self.dfs_acyclic(edge.target, colour)?,
            }
        }
        colour.insert(node, Colour::Black);
        Ok(())
    }

    /// Every node must be reachable from the root by forward traversal.
    fn check_reachable(&self, keys: &[NarrativeNodeId]) -> Result<(), NarrativeError> {
        let mut reachable: BTreeSet<NarrativeNodeId> = BTreeSet::new();
        let mut frontier = vec![self.root];
        while let Some(current) = frontier.pop() {
            if !reachable.insert(current) {
                continue;
            }
            for edge in &self.nodes[current].out {
                if !reachable.contains(&edge.target) {
                    frontier.push(edge.target);
                }
            }
        }
        for &key in keys {
            if !reachable.contains(&key) {
                return Err(NarrativeError::Unreachable(key));
            }
        }
        Ok(())
    }

    /// Every node must reach some ending, found by reverse traversal from endings.
    fn check_reaches_ending(
        &self,
        keys: &[NarrativeNodeId],
        in_edges: &BTreeMap<NarrativeNodeId, Vec<NarrativeNodeId>>,
    ) -> Result<(), NarrativeError> {
        let mut reaches: BTreeSet<NarrativeNodeId> = BTreeSet::new();
        let mut frontier: Vec<NarrativeNodeId> = self
            .endings
            .iter()
            .copied()
            .filter(|ending| self.nodes.contains_key(*ending))
            .collect();
        while let Some(current) = frontier.pop() {
            if !reaches.insert(current) {
                continue;
            }
            if let Some(predecessors) = in_edges.get(&current) {
                for &predecessor in predecessors {
                    if !reaches.contains(&predecessor) {
                        frontier.push(predecessor);
                    }
                }
            }
        }
        for &key in keys {
            if !reaches.contains(&key) {
                return Err(NarrativeError::DeadEnd(key));
            }
        }
        Ok(())
    }

    /// The endings set must be exactly the out-degree-0 nodes.
    fn check_endings(&self, keys: &[NarrativeNodeId]) -> Result<(), NarrativeError> {
        for &key in keys {
            let is_terminal = self.nodes[key].out.is_empty();
            let is_ending = self.endings.contains(&key);
            if is_terminal && !is_ending {
                return Err(NarrativeError::UnregisteredEnding(key));
            }
            if is_ending && !is_terminal {
                return Err(NarrativeError::NonTerminalEnding(key));
            }
        }
        // A registered ending that is not even a node (so absent from `keys`).
        for &ending in &self.endings {
            if !self.nodes.contains_key(ending) {
                return Err(NarrativeError::NonTerminalEnding(ending));
            }
        }
        Ok(())
    }

    /// Each node's declared kind must agree with its in/out degree.
    fn check_kinds(
        &self,
        keys: &[NarrativeNodeId],
        in_edges: &BTreeMap<NarrativeNodeId, Vec<NarrativeNodeId>>,
    ) -> Result<(), NarrativeError> {
        for &key in keys {
            let node = &self.nodes[key];
            let in_degree = in_edges.get(&key).map_or(0, Vec::len);
            let out_degree = node.out.len();
            let consistent = match node.kind {
                NodeKind::Origin => in_degree == 0,
                NodeKind::Ending => out_degree == 0,
                NodeKind::Branch => out_degree > 1,
                NodeKind::Merge => in_degree > 1,
                NodeKind::Beat => in_degree <= 1 && out_degree == 1,
            };
            if !consistent {
                return Err(NarrativeError::KindMismatch(key));
            }
        }
        Ok(())
    }
}
