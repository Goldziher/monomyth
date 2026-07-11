//! Integration tests for the narrative edit vocabulary.
//!
//! Every op is exercised against the public API only: `apply_edit` (raw, single) and
//! `apply_edits` (transactional). Fixtures are built up from the crate's
//! `single_node_structure` doc fixture via the edit ops themselves, so the tests also
//! double as end-to-end evidence that the surface can construct a valid graph.

use monomyth_core::doc_support::single_node_structure;
use monomyth_core::{
    EdgeKind, EditError, EditOutcome, NarrativeEdit, NarrativeNodeId, NarrativeStructure, NodeKind,
    NodeSpec, Weight,
};
use monomyth_frameworks::{MonomythStage, MotifClass, PoltiSituation, ProppFunction};

/// Unwrap a `NodeAdded` outcome or panic with a clear message.
fn added(outcome: EditOutcome) -> NarrativeNodeId {
    match outcome {
        EditOutcome::NodeAdded(id) => id,
        EditOutcome::Applied => panic!("expected a NodeAdded outcome, got Applied"),
    }
}

/// A spec with an arbitrary stage; edit tests never depend on the stage choice.
fn spec(label: &str) -> NodeSpec {
    NodeSpec::new(
        label,
        MonomythStage::TheRoadOfTrials,
        format!("hint for {label}"),
    )
}

/// The serialized form of a structure — the only way to compare two structures for
/// equality, since `NarrativeStructure` holds a `SlotMap` and is not `PartialEq`.
fn snapshot(structure: &NarrativeStructure) -> String {
    serde_json::to_string(structure).expect("a structure serializes")
}

/// A linear `root -> a -> b -> end` chain with all four ids returned in order.
fn linear_chain() -> (NarrativeStructure, [NarrativeNodeId; 4]) {
    let mut structure = single_node_structure();
    let root = structure.root();
    let end = added(
        structure
            .apply_edit(&NarrativeEdit::AddNode { spec: spec("end") })
            .expect("add end"),
    );
    structure
        .apply_edits(&[
            NarrativeEdit::Connect {
                source: root,
                target: end,
                kind: EdgeKind::Sequence,
            },
            NarrativeEdit::MarkEnding { node: end },
            NarrativeEdit::UnmarkEnding { node: root },
        ])
        .expect("wire root -> end");
    let a = added(
        structure
            .apply_edits(&[NarrativeEdit::InsertBeat {
                source: root,
                target: end,
                spec: spec("a"),
            }])
            .expect("insert a")[0],
    );
    let b = added(
        structure
            .apply_edits(&[NarrativeEdit::InsertBeat {
                source: a,
                target: end,
                spec: spec("b"),
            }])
            .expect("insert b")[0],
    );
    structure.validate().expect("chain is valid");
    (structure, [root, a, b, end])
}

/// A `root =(Seq)=> a -> end`, `root =(Choice)=> b -> end` diamond.
fn diamond() -> (NarrativeStructure, [NarrativeNodeId; 4]) {
    let mut structure = single_node_structure();
    let root = structure.root();
    let end = added(
        structure
            .apply_edit(&NarrativeEdit::AddNode { spec: spec("end") })
            .expect("add end"),
    );
    structure
        .apply_edits(&[
            NarrativeEdit::Connect {
                source: root,
                target: end,
                kind: EdgeKind::Sequence,
            },
            NarrativeEdit::MarkEnding { node: end },
            NarrativeEdit::UnmarkEnding { node: root },
        ])
        .expect("wire root -> end");
    let a = added(
        structure
            .apply_edits(&[NarrativeEdit::InsertBeat {
                source: root,
                target: end,
                spec: spec("a"),
            }])
            .expect("insert a")[0],
    );
    let b = added(
        structure
            .apply_edit(&NarrativeEdit::AddNode { spec: spec("b") })
            .expect("add b"),
    );
    structure
        .apply_edits(&[
            NarrativeEdit::Connect {
                source: root,
                target: b,
                kind: EdgeKind::Choice,
            },
            NarrativeEdit::Connect {
                source: b,
                target: end,
                kind: EdgeKind::Sequence,
            },
        ])
        .expect("wire the choice branch");
    structure.validate().expect("diamond is valid");
    (structure, [root, a, b, end])
}

/// The out-edge kind for `source -> target`, if present.
fn edge_kind(
    structure: &NarrativeStructure,
    source: NarrativeNodeId,
    target: NarrativeNodeId,
) -> Option<EdgeKind> {
    structure
        .node(source)?
        .out
        .iter()
        .find(|edge| edge.target == target)
        .map(|edge| edge.kind)
}

#[test]
fn add_node_returns_a_fresh_id() {
    let mut structure = single_node_structure();
    let outcome = structure
        .apply_edit(&NarrativeEdit::AddNode { spec: spec("beat") })
        .expect("add succeeds");
    let id = added(outcome);
    assert!(
        structure.node(id).is_some(),
        "the new node must be retrievable"
    );
    assert_eq!(structure.node(id).unwrap().label, "beat");
}

#[test]
fn insert_beat_splices_between_source_and_target() {
    let (structure, [root, a, b, end]) = linear_chain();
    assert_eq!(structure.spine(), vec![root, a, b, end]);
    assert_eq!(edge_kind(&structure, root, a), Some(EdgeKind::Sequence));
    assert_eq!(edge_kind(&structure, a, b), Some(EdgeKind::Sequence));
    assert_eq!(edge_kind(&structure, b, end), Some(EdgeKind::Sequence));
}

#[test]
fn insert_beat_on_a_choice_edge_preserves_the_choice_kind() {
    let (mut structure, [root, _a, b, _end]) = diamond();
    let mid = added(
        structure
            .apply_edits(&[NarrativeEdit::InsertBeat {
                source: root,
                target: b,
                spec: spec("mid"),
            }])
            .expect("insert on the choice edge")[0],
    );
    assert_eq!(edge_kind(&structure, root, mid), Some(EdgeKind::Choice));
    assert_eq!(edge_kind(&structure, mid, b), Some(EdgeKind::Sequence));
    structure
        .validate()
        .expect("still valid after choice-edge splice");
}

#[test]
fn remove_beat_heals_predecessor_to_successor() {
    let (mut structure, [root, a, b, end]) = linear_chain();
    let before = structure.nodes.len();
    structure
        .apply_edits(&[NarrativeEdit::RemoveBeat { node: a }])
        .expect("remove the beat a");
    assert_eq!(structure.nodes.len(), before - 1);
    assert!(structure.node(a).is_none(), "a is gone");
    assert_eq!(edge_kind(&structure, root, b), Some(EdgeKind::Sequence));
    assert_eq!(structure.spine(), vec![root, b, end]);
}

#[test]
fn remove_beat_rejects_the_root() {
    let (mut structure, [root, ..]) = linear_chain();
    let error = structure
        .apply_edit(&NarrativeEdit::RemoveBeat { node: root })
        .expect_err("root is not a removable beat");
    assert_eq!(error, EditError::CannotRemoveRoot);
}

#[test]
fn remove_beat_rejects_a_branch_node() {
    let (mut structure, [_root, a, _b, end]) = linear_chain();
    let side = added(
        structure
            .apply_edit(&NarrativeEdit::AddNode { spec: spec("side") })
            .expect("add side"),
    );
    structure
        .apply_edits(&[
            NarrativeEdit::Connect {
                source: a,
                target: side,
                kind: EdgeKind::Choice,
            },
            NarrativeEdit::Connect {
                source: side,
                target: end,
                kind: EdgeKind::Sequence,
            },
        ])
        .expect("wire the side branch");
    let error = structure
        .apply_edit(&NarrativeEdit::RemoveBeat { node: a })
        .expect_err("a branch is not a plain beat");
    assert_eq!(error, EditError::NotABeat(a));
}

#[test]
fn move_beat_relocates_a_beat_and_heals_the_gap() {
    let (mut structure, [root, a, b, end]) = linear_chain();
    let outcome = structure
        .apply_edits(&[NarrativeEdit::MoveBeat {
            node: b,
            source: root,
            target: a,
        }])
        .expect("move b onto root -> a")[0];
    let moved = added(outcome);
    assert!(structure.node(b).is_none(), "the original b id is retired");
    assert_eq!(structure.spine(), vec![root, moved, a, end]);
    assert_eq!(structure.node(moved).unwrap().label, "b");
}

#[test]
fn connect_and_disconnect_round_trip() {
    let (mut structure, [root, _a, _b, end]) = diamond();
    let before = snapshot(&structure);
    structure
        .apply_edits(&[NarrativeEdit::Connect {
            source: root,
            target: end,
            kind: EdgeKind::Choice,
        }])
        .expect("add shortcut");
    assert_eq!(edge_kind(&structure, root, end), Some(EdgeKind::Choice));
    structure
        .apply_edits(&[NarrativeEdit::Disconnect {
            source: root,
            target: end,
        }])
        .expect("drop shortcut");
    assert_eq!(
        snapshot(&structure),
        before,
        "round trip restores the structure"
    );
}

#[test]
fn connect_rejects_a_duplicate_edge() {
    let (mut structure, [root, a, ..]) = linear_chain();
    let error = structure
        .apply_edit(&NarrativeEdit::Connect {
            source: root,
            target: a,
            kind: EdgeKind::Choice,
        })
        .expect_err("root -> a already exists");
    assert_eq!(
        error,
        EditError::EdgeExists {
            source: root,
            target: a
        }
    );
}

#[test]
fn retarget_edge_repoints_while_preserving_kind_and_guard() {
    let (mut structure, [_root, a, b, end]) = diamond();
    structure
        .apply_edits(&[
            NarrativeEdit::SetEdgeGuard {
                source: a,
                target: end,
                guard: Some("torch_lit".to_owned()),
            },
            NarrativeEdit::RetargetEdge {
                source: a,
                old_target: end,
                new_target: b,
            },
        ])
        .expect("retarget a -> end onto a -> b");
    assert_eq!(edge_kind(&structure, a, b), Some(EdgeKind::Sequence));
    let guard = structure
        .node(a)
        .unwrap()
        .out
        .iter()
        .find(|edge| edge.target == b)
        .and_then(|edge| edge.guard.clone());
    assert_eq!(guard.as_deref(), Some("torch_lit"), "the guard rides along");
    assert!(
        edge_kind(&structure, a, end).is_none(),
        "the old edge is gone"
    );
}

#[test]
fn relabel_and_attribute_setters_take_effect() {
    let (mut structure, [_root, a, ..]) = linear_chain();
    let function = *ProppFunction::all()
        .first()
        .expect("a propp function exists");
    let situation = *PoltiSituation::all()
        .first()
        .expect("a polti situation exists");
    let motif = *MotifClass::all().first().expect("a motif class exists");
    structure
        .apply_edits(&[
            NarrativeEdit::RelabelNode {
                node: a,
                label: "renamed".to_owned(),
            },
            NarrativeEdit::SetNodeStage {
                node: a,
                stage: MonomythStage::BellyOfTheWhale.into(),
            },
            NarrativeEdit::SetNodeSituation {
                node: a,
                situation: Some(situation.into()),
            },
            NarrativeEdit::SetNodeFunctions {
                node: a,
                functions: [(function, Weight::FULL)].into_iter().collect(),
            },
            NarrativeEdit::SetNodeMotifs {
                node: a,
                motifs: [(motif, Weight::FULL)].into_iter().collect(),
            },
        ])
        .expect("apply the attribute edits");
    let node = structure.node(a).expect("a exists");
    assert_eq!(node.label, "renamed");
    assert_eq!(*node.stage.primary(), MonomythStage::BellyOfTheWhale);
    assert_eq!(
        node.situation.as_ref().map(|s| *s.primary()),
        Some(situation)
    );
    assert!(node.functions.contains(&function));
    assert!(node.motifs.contains(&motif));
}

#[test]
fn set_edge_kind_changes_only_the_kind() {
    let (mut structure, [root, a, ..]) = linear_chain();
    structure
        .apply_edits(&[NarrativeEdit::SetEdgeKind {
            source: root,
            target: a,
            kind: EdgeKind::Choice,
        }])
        .expect("set edge kind");
    assert_eq!(edge_kind(&structure, root, a), Some(EdgeKind::Choice));
}

#[test]
fn mark_and_unmark_ending_move_the_terminal_set() {
    let (mut structure, [_root, _a, _b, end]) = linear_chain();
    let coda = added(
        structure
            .apply_edit(&NarrativeEdit::AddNode { spec: spec("coda") })
            .expect("add coda"),
    );
    structure
        .apply_edits(&[
            NarrativeEdit::Connect {
                source: end,
                target: coda,
                kind: EdgeKind::Sequence,
            },
            NarrativeEdit::MarkEnding { node: coda },
            NarrativeEdit::UnmarkEnding { node: end },
        ])
        .expect("extend the ending");
    assert!(structure.is_ending(coda));
    assert!(!structure.is_ending(end));
    assert_eq!(structure.node(coda).unwrap().kind, NodeKind::Ending);
}

#[test]
fn a_failing_batch_leaves_the_structure_byte_identical() {
    let (mut structure, [root, a, ..]) = linear_chain();
    let before = snapshot(&structure);
    let result = structure.apply_edits(&[
        NarrativeEdit::RelabelNode {
            node: a,
            label: "doomed".to_owned(),
        },
        NarrativeEdit::Connect {
            source: a,
            target: root,
            kind: EdgeKind::Choice,
        },
    ]);
    assert!(
        matches!(result, Err(EditError::Validation(_))),
        "cycle must be rejected"
    );
    assert_eq!(snapshot(&structure), before, "rollback restores every byte");
}

#[test]
fn a_local_precondition_failure_rolls_the_whole_batch_back() {
    let (mut structure, [_root, a, ..]) = linear_chain();
    let ghost = added(
        structure
            .apply_edit(&NarrativeEdit::AddNode {
                spec: spec("ghost"),
            })
            .expect("add ghost"),
    );
    structure
        .apply_edit(&NarrativeEdit::RemoveNode { node: ghost })
        .expect("retire ghost");
    let before = snapshot(&structure);

    let result = structure.apply_edits(&[
        NarrativeEdit::RelabelNode {
            node: a,
            label: "doomed".to_owned(),
        },
        NarrativeEdit::RelabelNode {
            node: ghost,
            label: "nowhere".to_owned(),
        },
    ]);
    assert!(matches!(result, Err(EditError::NodeNotFound(_))));
    assert_eq!(
        snapshot(&structure),
        before,
        "the earlier relabel is undone too"
    );
    assert_ne!(structure.node(a).unwrap().label, "doomed");
}

#[test]
fn recompute_keeps_a_forking_root_as_origin() {
    let (structure, [root, ..]) = diamond();
    assert!(structure.is_fork(root), "the root branches");
    assert_eq!(structure.node(root).unwrap().kind, NodeKind::Origin);
}
