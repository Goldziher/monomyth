//! End-to-end extraction score against the committed Odyssey benchmark fixture
//! (ADR-0023 Phase B4b).
//!
//! Runs the structure-preserving [`StageReclassifyingExtractor`] over the real
//! gold fixture as a skeleton, then scores the re-classified world against the
//! gold with the eval harness's [`AlignmentScorer`]. The extractor here is driven
//! by a deterministic, content-derived fake retriever (no LLM, no vector store,
//! no network), so the whole pipeline is hermetic and its `Report` fingerprint
//! can be pinned exactly the way the fixture bytes and the scorer's own goldens
//! are elsewhere.
//!
//! Because the extractor preserves structure and only re-derives the scored
//! `stage` axis, the structural precision/recall/F1 are necessarily perfect; the
//! classification quality (which, under a semantically-meaningless fake
//! retriever, is arbitrary but deterministic) surfaces in the `stage` axis and is
//! captured by the pinned fingerprint.

use async_trait::async_trait;

use monomyth_contracts::{Extractor, PassageRetriever, RetrievalError, ScoredHit};
use monomyth_core::World;
use monomyth_eval::{AlignmentScorer, Benchmark, Scorer, report_fingerprint};
use monomyth_extract::{RagSoftmaxClassifier, StageReclassifyingExtractor};

/// The pinned FNV-1a hash of `artifacts/benchmarks/greek/odyssey.json`'s exact
/// committed bytes, mirrored from `monomyth-eval`'s `ODYSSEY_CONTENT_HASH`. Both
/// must be updated together if the fixture is intentionally revised.
const ODYSSEY_CONTENT_HASH: u64 = 0x60d0_92be_d277_40f9;

/// The `Report` fingerprint of extracting stages over the Odyssey skeleton with
/// [`HashRetriever`] and scoring against the gold. Captured from a real run (see
/// the module doc); regenerate deliberately if the classifier, the fixture, or
/// the scorer changes.
const ODYSSEY_EXTRACTION_FINGERPRINT: u64 = 0x843c_47cf_9835_88ee;

/// A deterministic, hermetic [`PassageRetriever`]: it returns exactly one hit per
/// query whose score is a stable, content-derived function of the query bytes.
///
/// This is a semantically-meaningless retriever — it does not model relevance —
/// but it is fully deterministic, so it exercises the real extraction+scoring
/// pipeline end-to-end and produces a stable, pinnable `Report`. Every distinct
/// fused query yields a distinct-but-reproducible score, which is exactly what a
/// determinism/plumbing baseline needs.
struct HashRetriever;

#[async_trait]
impl PassageRetriever for HashRetriever {
    async fn retrieve_scores(
        &self,
        query: &str,
        _top_k: u32,
    ) -> Result<Vec<ScoredHit>, RetrievalError> {
        // A stable byte-sum mapped into 0.0..1.0. `u32` accumulation cannot
        // overflow for any realistic query length, and the modulo keeps the
        // score in a fixed range independent of query length.
        let checksum: u32 = query.bytes().map(u32::from).sum::<u32>() % 1000;
        #[allow(
            clippy::cast_precision_loss,
            reason = "checksum is in 0..1000, exactly representable as f32"
        )]
        let score = checksum as f32 / 1000.0;
        Ok(vec![ScoredHit { score }])
    }
}

fn load_odyssey() -> World {
    let json = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../artifacts/benchmarks/greek/odyssey.json"),
    )
    .expect("artifacts/benchmarks/greek/odyssey.json is committed");
    Benchmark::load(&json, ODYSSEY_CONTENT_HASH)
        .expect("the committed fixture loads at its pinned hash")
        .world
}

#[tokio::test]
async fn extracting_over_the_odyssey_skeleton_preserves_structure_and_pins_a_fingerprint() {
    let gold = load_odyssey();

    let extractor = StageReclassifyingExtractor::new(RagSoftmaxClassifier::new(HashRetriever));
    let predicted = extractor
        .extract(&gold)
        .await
        .expect("re-classifying stages over a valid skeleton yields a valid world");

    predicted
        .validate()
        .expect("the extracted world is structurally valid");

    let report = AlignmentScorer.score(&gold, &predicted);

    // Structure is preserved (only the scored stage axis is re-derived), so the
    // node alignment is perfect regardless of classification quality.
    assert!((report.structural_precision - 1.0).abs() < 1e-9);
    assert!((report.structural_recall - 1.0).abs() < 1e-9);
    assert!((report.structural_f1 - 1.0).abs() < 1e-9);
    assert!(report.alignment.unmatched_gold.is_empty());
    assert!(report.alignment.unmatched_predicted.is_empty());

    let fingerprint = report_fingerprint(&report);
    assert_eq!(
        fingerprint, ODYSSEY_EXTRACTION_FINGERPRINT,
        "extraction fingerprint drifted: got {fingerprint:#018x}, expected {ODYSSEY_EXTRACTION_FINGERPRINT:#018x}"
    );
}

#[tokio::test]
async fn extraction_is_deterministic_across_runs() {
    let gold = load_odyssey();
    let extractor = StageReclassifyingExtractor::new(RagSoftmaxClassifier::new(HashRetriever));

    let first = extractor.extract(&gold).await.expect("first run succeeds");
    let second = extractor.extract(&gold).await.expect("second run succeeds");

    let first_report = AlignmentScorer.score(&gold, &first);
    let second_report = AlignmentScorer.score(&gold, &second);

    assert_eq!(
        report_fingerprint(&first_report),
        report_fingerprint(&second_report),
        "two extraction+score runs must fingerprint identically"
    );
}
