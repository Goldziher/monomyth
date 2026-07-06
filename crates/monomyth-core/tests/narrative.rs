//! Behavioural tests over the [`NarrativeStructure`] public contract: the
//! validation invariants and the read-only query helpers.

use std::collections::BTreeSet;

use monomyth_core::{
    Content, ContentKind, ContentPrompt, EdgeKind, NarrativeEdge, NarrativeError, NarrativeNode,
    NarrativeNodeId, NarrativeStructure, NodeKind,
};
use monomyth_frameworks::MonomythStage;
use slotmap::SlotMap;

/// An empty synopsis slot hinted with `hint`.
fn synopsis(hint: &str) -> Content {
    Content::empty(ContentPrompt::new(ContentKind::Synopsis, hint))
}

/// An empty choice-label slot.
fn label() -> Content {
    Content::empty(ContentPrompt::new(ContentKind::Choice, ""))
}

/// A linear three-node trunk `origin -> beat -> ending`, all Sequence edges.
///
/// Returns the structure plus the three ids in trunk order.
fn linear_trunk() -> (NarrativeStructure, [NarrativeNodeId; 3]) {
    let mut nodes = SlotMap::with_key();
    let origin = nodes.insert(NarrativeNode::new(
        "CallToAdventure",
        NodeKind::Origin,
        MonomythStage::CallToAdventure,
        synopsis("origin"),
    ));
    let beat = nodes.insert(NarrativeNode::new(
        "TheRoadOfTrials",
        NodeKind::Beat,
        MonomythStage::TheRoadOfTrials,
        synopsis("beat"),
    ));
    let ending = nodes.insert(NarrativeNode::new(
        "FreedomToLive",
        NodeKind::Ending,
        MonomythStage::FreedomToLive,
        synopsis("ending"),
    ));
    nodes[origin]
        .out
        .push(NarrativeEdge::new(beat, EdgeKind::Sequence, label()));
    nodes[beat]
        .out
        .push(NarrativeEdge::new(ending, EdgeKind::Sequence, label()));
    let structure = NarrativeStructure {
        nodes,
        root: origin,
        endings: BTreeSet::from([ending]),
    };
    (structure, [origin, beat, ending])
}

/// A reconverging diamond: `origin` branches to `left` and `right`, both merging
/// at `merge`, which ends. Returns the structure and its ids.
fn diamond() -> (NarrativeStructure, [NarrativeNodeId; 4]) {
    let mut nodes = SlotMap::with_key();
    let origin = nodes.insert(NarrativeNode::new(
        "origin",
        NodeKind::Branch,
        MonomythStage::CallToAdventure,
        synopsis("origin"),
    ));
    let left = nodes.insert(NarrativeNode::new(
        "left",
        NodeKind::Beat,
        MonomythStage::RefusalOfTheCall,
        synopsis("left"),
    ));
    let right = nodes.insert(NarrativeNode::new(
        "right",
        NodeKind::Beat,
        MonomythStage::SupernaturalAid,
        synopsis("right"),
    ));
    let merge = nodes.insert(NarrativeNode::new(
        "merge",
        NodeKind::Merge,
        MonomythStage::FreedomToLive,
        synopsis("merge"),
    ));
    nodes[origin]
        .out
        .push(NarrativeEdge::new(left, EdgeKind::Choice, label()));
    nodes[origin]
        .out
        .push(NarrativeEdge::new(right, EdgeKind::Choice, label()));
    nodes[left]
        .out
        .push(NarrativeEdge::new(merge, EdgeKind::Sequence, label()));
    nodes[right]
        .out
        .push(NarrativeEdge::new(merge, EdgeKind::Sequence, label()));
    let structure = NarrativeStructure {
        nodes,
        root: origin,
        endings: BTreeSet::from([merge]),
    };
    (structure, [origin, left, right, merge])
}

#[test]
fn should_validate_a_linear_trunk() {
    let (structure, _) = linear_trunk();
    assert_eq!(structure.validate(), Ok(()));
}

#[test]
fn should_validate_a_reconverging_diamond() {
    let (structure, _) = diamond();
    assert_eq!(structure.validate(), Ok(()));
}

#[test]
fn should_reject_missing_root() {
    let (mut structure, [_, _, ending]) = linear_trunk();
    let ghost = structure.nodes.insert(NarrativeNode::new(
        "ghost",
        NodeKind::Ending,
        MonomythStage::FreedomToLive,
        synopsis("ghost"),
    ));
    structure.nodes.remove(ghost);
    structure.root = ghost;
    // Keep endings well-formed so MissingRoot is the reported failure.
    let _ = ending;
    assert_eq!(
        structure.validate(),
        Err(NarrativeError::MissingRoot(ghost))
    );
}

#[test]
fn should_reject_dangling_edge() {
    let (mut structure, [origin, beat, ending]) = linear_trunk();
    structure.nodes.remove(ending);
    structure.endings = BTreeSet::from([beat]);
    // `beat` still points at the removed `ending`.
    assert_eq!(
        structure.validate(),
        Err(NarrativeError::DanglingEdge {
            node: beat,
            target: ending,
        }),
    );
    let _ = origin;
}

#[test]
fn should_reject_duplicate_edge() {
    let (mut structure, [origin, beat, _]) = linear_trunk();
    // A second, identical edge from origin to beat.
    structure.nodes[origin]
        .out
        .push(NarrativeEdge::new(beat, EdgeKind::Sequence, label()));
    assert_eq!(
        structure.validate(),
        Err(NarrativeError::DuplicateEdge {
            node: origin,
            target: beat,
        }),
    );
}

#[test]
fn should_reject_orphan_source() {
    let (mut structure, _) = linear_trunk();
    // A second in-degree-0 node with a path to an ending.
    let ending = *structure.endings.iter().next().expect("one ending");
    let orphan = structure.nodes.insert(NarrativeNode::new(
        "orphan",
        NodeKind::Origin,
        MonomythStage::CallToAdventure,
        synopsis("orphan"),
    ));
    structure.nodes[orphan]
        .out
        .push(NarrativeEdge::new(ending, EdgeKind::Sequence, label()));
    assert_eq!(
        structure.validate(),
        Err(NarrativeError::OrphanSource(orphan)),
    );
}

#[test]
fn should_reject_a_cycle() {
    let (mut structure, [origin, _, ending]) = linear_trunk();
    // A back-edge from the ending to the origin closes a cycle (and makes the
    // former ending non-terminal, but the cycle is detected first).
    structure.endings = BTreeSet::new();
    structure.nodes[ending]
        .out
        .push(NarrativeEdge::new(origin, EdgeKind::Sequence, label()));
    assert_eq!(structure.validate(), Err(NarrativeError::Cycle(origin)));
}

#[test]
fn should_reject_unregistered_ending() {
    let (mut structure, [_, beat, _]) = linear_trunk();
    // Add a terminal node reachable from `beat` but not registered as an ending.
    let extra = structure.nodes.insert(NarrativeNode::new(
        "extra",
        NodeKind::Ending,
        MonomythStage::MasterOfTheTwoWorlds,
        synopsis("extra"),
    ));
    structure.nodes[beat]
        .out
        .push(NarrativeEdge::new(extra, EdgeKind::Choice, label()));
    assert_eq!(
        structure.validate(),
        Err(NarrativeError::UnregisteredEnding(extra)),
    );
}

#[test]
fn should_reject_non_terminal_ending() {
    let (mut structure, [origin, beat, ending]) = linear_trunk();
    // Register the interior `beat` (which has an out-edge) as an ending.
    structure.endings = BTreeSet::from([beat, ending]);
    let _ = origin;
    assert_eq!(
        structure.validate(),
        Err(NarrativeError::NonTerminalEnding(beat)),
    );
}

#[test]
fn should_reject_empty_endings() {
    let (mut structure, _) = linear_trunk();
    structure.endings = BTreeSet::new();
    assert_eq!(structure.validate(), Err(NarrativeError::NoEndings));
}

#[test]
fn should_reject_kind_mismatch() {
    let (mut structure, [origin, _, _]) = linear_trunk();
    // The origin has out-degree 1, so declaring it a Branch (out-degree > 1) lies.
    structure.nodes[origin].kind = NodeKind::Branch;
    assert_eq!(
        structure.validate(),
        Err(NarrativeError::KindMismatch(origin)),
    );
}

#[test]
fn spine_follows_the_primary_sequence_edge() {
    let (structure, [origin, beat, ending]) = linear_trunk();
    assert_eq!(structure.spine(), vec![origin, beat, ending]);
}

#[test]
fn spine_of_a_diamond_takes_the_first_edge() {
    let (structure, [origin, left, _, merge]) = diamond();
    // Both branch edges are Choice, so the spine follows edge index 0 (`left`).
    assert_eq!(structure.spine(), vec![origin, left, merge]);
}

#[test]
fn children_lists_out_edge_targets() {
    let (structure, [origin, left, right, _]) = diamond();
    let children: Vec<_> = structure.children(origin).collect();
    assert_eq!(children, vec![left, right]);
}

#[test]
fn is_fork_and_is_merge_reflect_topology() {
    let (structure, [origin, left, _, merge]) = diamond();
    assert!(structure.is_fork(origin), "origin branches");
    assert!(!structure.is_fork(left), "left is a plain beat");
    assert!(structure.is_merge(merge), "merge joins two branches");
    assert!(!structure.is_merge(origin), "origin has no predecessor");
}

#[test]
fn topological_order_is_a_stable_linear_extension() {
    let (structure, [origin, left, right, merge]) = diamond();
    let order = structure.topological_order();
    assert_eq!(order.len(), 4, "every node appears once");
    let position = |id| order.iter().position(|&n| n == id).expect("node present");
    assert!(position(origin) < position(left));
    assert!(position(origin) < position(right));
    assert!(position(left) < position(merge));
    assert!(position(right) < position(merge));
}

#[test]
fn endings_and_is_ending_agree() {
    let (structure, [origin, _, ending]) = linear_trunk();
    let endings: Vec<_> = structure.endings().collect();
    assert_eq!(endings, vec![ending]);
    assert!(structure.is_ending(ending));
    assert!(!structure.is_ending(origin));
}
