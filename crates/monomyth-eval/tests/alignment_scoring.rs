//! Golden `report_fingerprint` integration test for [`monomyth_eval::AlignmentScorer`]
//! (ADR-0023 Phase B3) against the real, committed Odyssey fixture.
//!
//! Complements the hand-crafted near-miss unit tests in
//! `crates/monomyth-eval/src/alignment.rs`, which pin the aligner's exact
//! behavior on small (2-4 node) cases; this test exercises the same scorer
//! against the full 13-node fixture used elsewhere in the eval harness
//! (`tests/odyssey_fixture.rs`), pinning the resulting `Report` fingerprint the
//! same way `ODYSSEY_CONTENT_HASH` pins the fixture bytes.

use monomyth_core::{NarrativeEdit, World};
use monomyth_eval::{AlignmentScorer, Benchmark, Scorer, report_fingerprint};

/// The pinned FNV-1a hash of `artifacts/benchmarks/greek/odyssey.json`'s exact
/// committed bytes, mirrored from `tests/odyssey_fixture.rs`'s
/// `ODYSSEY_CONTENT_HASH`. Kept as a local copy (not `pub` in that file) since
/// this is a separate test binary; both must be updated together if the
/// fixture is intentionally revised.
const ODYSSEY_CONTENT_HASH: u64 = 0x60d0_92be_d277_40f9;

/// The `Report` fingerprint of scoring the committed Odyssey fixture against an
/// exact clone of itself: perfect alignment, P = R = F1 = 1.0. Obtained by
/// running this test once, reading the printed/panicked value, and hardcoding
/// it here — the same discipline `ODYSSEY_CONTENT_HASH` uses in
/// `tests/odyssey_fixture.rs`.
const ODYSSEY_SELF_SCORE_FINGERPRINT: u64 = 0xe852_73b6_ce90_fe55;

/// The `Report` fingerprint of scoring the committed Odyssey fixture against a
/// perturbed copy with the `GoddessAndTemptress` node's `stage` primary
/// replaced by `MonomythStage::Apotheosis` (a stage the real fixture never
/// realizes — see `odyssey_fixture.rs`'s "explicitly omitted" note — chosen
/// specifically because it shares no support with the original primary or its
/// `WomanAsTemptress` alternative, guaranteeing a real, non-trivial score
/// change rather than a coincidental near-miss).
const ODYSSEY_RELABELED_SCORE_FINGERPRINT: u64 = 0x9ef1_cc0b_0c7a_60d8;

fn artifacts_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../artifacts/benchmarks/greek")
}

fn committed_world_json() -> String {
    std::fs::read_to_string(artifacts_dir().join("odyssey.json"))
        .expect("artifacts/benchmarks/greek/odyssey.json is committed")
}

fn load_odyssey() -> World {
    let json = committed_world_json();
    Benchmark::load(&json, ODYSSEY_CONTENT_HASH)
        .expect("the committed fixture loads at its pinned hash")
        .world
}

/// The Odyssey fixture scored against an exact clone of itself must align
/// every node to its counterpart, score P = R = F1 = 1.0, and fingerprint to
/// the pinned [`ODYSSEY_SELF_SCORE_FINGERPRINT`].
#[test]
fn odyssey_self_score_should_be_perfect_and_match_the_pinned_fingerprint() {
    let gold = load_odyssey();
    let predicted = load_odyssey();

    let report = AlignmentScorer.score(&gold, &predicted);

    assert_eq!(
        report.alignment.matches.len(),
        gold.story.structure.nodes.len(),
        "every gold node must align to its counterpart in an identical predicted world"
    );
    assert!(report.alignment.unmatched_gold.is_empty());
    assert!(report.alignment.unmatched_predicted.is_empty());
    assert!((report.structural_precision - 1.0).abs() < 1e-9);
    assert!((report.structural_recall - 1.0).abs() < 1e-9);
    assert!((report.structural_f1 - 1.0).abs() < 1e-9);

    let fingerprint = report_fingerprint(&report);
    assert_eq!(
        fingerprint, ODYSSEY_SELF_SCORE_FINGERPRINT,
        "self-score fingerprint drifted: got {fingerprint:#018x}, expected {ODYSSEY_SELF_SCORE_FINGERPRINT:#018x}"
    );
}

/// The Odyssey fixture scored against a copy with the scored-split
/// `GoddessAndTemptress` node relabeled to `Apotheosis` must still align every
/// node (a single relabeled node is a partial mismatch, not a gap — see the
/// aligner's `relabeled_stage_should_still_align_despite_a_bad_substitution`
/// unit test), score a strictly lower `stage` axis than the self-score, keep
/// P = R = F1 = 1.0 (structural alignment is unaffected by a classification
/// error), and fingerprint to the pinned
/// [`ODYSSEY_RELABELED_SCORE_FINGERPRINT`] (necessarily different from
/// [`ODYSSEY_SELF_SCORE_FINGERPRINT`]).
#[test]
fn odyssey_relabeled_node_score_should_match_the_pinned_fingerprint() {
    let gold = load_odyssey();
    let mut predicted = load_odyssey();

    let goddess_node_id = predicted
        .story
        .structure
        .nodes
        .iter()
        .find(|(_, node)| node.label == "GoddessAndTemptress")
        .map(|(id, _)| id)
        .expect("the Odyssey fixture has a GoddessAndTemptress node");

    let relabel = NarrativeEdit::SetNodeStage {
        node: goddess_node_id,
        stage: monomyth_core::ScoredOne::new(monomyth_frameworks::MonomythStage::Apotheosis),
    };
    predicted
        .story
        .structure
        .apply_edits(&[relabel])
        .expect("relabeling a single node's stage keeps the structure valid");

    let report = AlignmentScorer.score(&gold, &predicted);

    assert_eq!(
        report.alignment.matches.len(),
        gold.story.structure.nodes.len(),
        "a relabeled node must still align, not be dropped as a gap"
    );
    assert!(report.alignment.unmatched_gold.is_empty());
    assert!(report.alignment.unmatched_predicted.is_empty());
    assert!((report.structural_precision - 1.0).abs() < 1e-9);
    assert!((report.structural_recall - 1.0).abs() < 1e-9);
    assert!((report.structural_f1 - 1.0).abs() < 1e-9);

    let self_report = AlignmentScorer.score(&gold, &gold);
    let stage_score = report.axes.get("stage").expect("stage axis present");
    let self_stage_score = self_report.axes.get("stage").expect("stage axis present");
    assert!(
        stage_score.histogram_intersection < self_stage_score.histogram_intersection,
        "relabeling one node must strictly lower the averaged stage histogram-intersection"
    );

    let fingerprint = report_fingerprint(&report);
    assert_eq!(
        fingerprint, ODYSSEY_RELABELED_SCORE_FINGERPRINT,
        "relabeled-score fingerprint drifted: got {fingerprint:#018x}, expected {ODYSSEY_RELABELED_SCORE_FINGERPRINT:#018x}"
    );
    assert_ne!(
        fingerprint, ODYSSEY_SELF_SCORE_FINGERPRINT,
        "a real score change must not fingerprint identically to the perfect self-score"
    );
}
