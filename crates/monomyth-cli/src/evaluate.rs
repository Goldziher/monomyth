//! The `eval` subcommand: score an extractor against a ground-truth benchmark
//! fixture (ADR-0023).
//!
//! This is the one place the extraction plane (`monomyth-extract`), the scoring
//! harness (`monomyth-eval`), and the knowledge layer (`monomyth-knowledge`) are
//! wired together. [`KnowledgeRetriever`] adapts the ship-gated retrieval seam to
//! the `monomyth-contracts` [`PassageRetriever`] the classifier programs against,
//! so the classifier itself never depends on the concrete store. The pure helpers
//! ([`parse_content_hash`], [`resolve_fixture`], [`validate_extractor`]) carry the
//! testable logic; [`run_eval`] owns the IO and async calls.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use serde::Deserialize;

use monomyth_contracts::{Extractor, PassageRetriever, RetrievalError, ScoredHit};
use monomyth_eval::{AlignmentScorer, Benchmark, Scorer, report_fingerprint};
use monomyth_extract::{RagSoftmaxClassifier, StageReclassifyingExtractor};
use monomyth_knowledge::{Knowledge, KnowledgeQuery};

/// The only extractor strategy available today. Kept as a named constant so the
/// allowlist in [`validate_extractor`] and the error message share one source.
const RAG_SOFTMAX: &str = "rag-softmax";

/// The benchmark registry (`artifacts/benchmarks/index.json`), deserialized down
/// to the fields `eval` needs. Unknown fields (`note`, `edit_script_path`, …) are
/// ignored by serde.
#[derive(Debug, Deserialize)]
struct BenchmarkIndex {
    fixtures: Vec<FixtureEntry>,
}

/// One registered benchmark fixture.
#[derive(Debug, Clone, Deserialize)]
struct FixtureEntry {
    /// The `--work` id.
    id: String,
    /// Fixture path, relative to the benchmarks directory.
    path: String,
    /// The classification axis this fixture is gold for (e.g. `campbell_macro`).
    axis: String,
    /// The pinned FNV-1a content hash, as a `0x`-prefixed hex string.
    content_hash: String,
}

/// Adapts `monomyth-knowledge`'s ship-gated retrieval to the
/// [`PassageRetriever`] seam a [`RagSoftmaxClassifier`] consumes.
///
/// This adapter is the production `PassageRetriever`; it deliberately lives with
/// its consumer (the CLI) rather than in `monomyth-extract`, so that crate stays
/// free of the xberg/ONNX backend and its tests stay hermetic.
pub(crate) struct KnowledgeRetriever {
    knowledge: Knowledge,
}

impl KnowledgeRetriever {
    /// Wrap an opened knowledge store.
    pub(crate) const fn new(knowledge: Knowledge) -> Self {
        Self { knowledge }
    }
}

#[async_trait]
impl PassageRetriever for KnowledgeRetriever {
    async fn retrieve_scores(
        &self,
        query: &str,
        top_k: u32,
    ) -> Result<Vec<ScoredHit>, RetrievalError> {
        // Surfaceable (ship-gated) retrieval: extraction priors must be drawn
        // from the same corpus a shippable query would see.
        let passages = self
            .knowledge
            .retrieve(KnowledgeQuery::surfaceable(query, top_k))
            .await
            .map_err(|error| RetrievalError(error.to_string()))?;
        Ok(passages
            .into_iter()
            .map(|passage| ScoredHit {
                score: passage.score,
            })
            .collect())
    }
}

/// Parse a `0x`-prefixed (or bare) hex string into a `u64` content hash.
///
/// # Errors
///
/// Fails if the digits after any `0x`/`0X` prefix are not valid base-16.
fn parse_content_hash(raw: &str) -> Result<u64> {
    let hex = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or(raw);
    u64::from_str_radix(hex, 16).with_context(|| format!("parsing content hash {raw:?}"))
}

/// Find the fixture registered under `work_id`, or error listing what is
/// available.
///
/// # Errors
///
/// Fails if no fixture has that id.
fn resolve_fixture(index: &BenchmarkIndex, work_id: &str) -> Result<FixtureEntry> {
    index
        .fixtures
        .iter()
        .find(|fixture| fixture.id == work_id)
        .cloned()
        .ok_or_else(|| {
            let available = index
                .fixtures
                .iter()
                .map(|fixture| fixture.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow!("unknown benchmark work {work_id:?}; available: [{available}]")
        })
}

/// Validate the requested extractor against the allowlist.
///
/// # Errors
///
/// Fails on any name outside the allowlist (currently just `rag-softmax`).
fn validate_extractor(name: &str) -> Result<()> {
    match name {
        RAG_SOFTMAX => Ok(()),
        other => bail!("unknown extractor {other:?}; available: [{RAG_SOFTMAX}]"),
    }
}

/// Handle `eval`: load a gold fixture, run the extractor over it as a skeleton,
/// score the result against gold, and emit a JSON report.
///
/// The gold world is loaded through [`Benchmark::load`], which verifies the
/// registry's pinned content hash before trusting the bytes. The re-classified
/// world is scored with the deterministic [`AlignmentScorer`]; the report and its
/// fingerprint go to `out` when given, otherwise to stdout, while a short human
/// summary goes to stderr so stdout stays clean JSON.
///
/// # Errors
///
/// Fails if the extractor name is unknown, the registry or fixture cannot be read
/// or parsed, the fixture's content hash does not match, the knowledge store
/// cannot be opened, extraction fails, or serialization/writing the report fails.
pub(crate) async fn run_eval(
    work: &str,
    extractor: &str,
    benchmarks_dir: &Path,
    out: Option<PathBuf>,
    db: &Path,
) -> Result<()> {
    validate_extractor(extractor)?;

    let index_path = benchmarks_dir.join("index.json");
    let index_json = std::fs::read_to_string(&index_path)
        .with_context(|| format!("reading benchmark registry {}", index_path.display()))?;
    let index: BenchmarkIndex = serde_json::from_str(&index_json)
        .with_context(|| format!("parsing benchmark registry {}", index_path.display()))?;

    let fixture = resolve_fixture(&index, work)?;
    let content_hash = parse_content_hash(&fixture.content_hash)?;

    let fixture_path = benchmarks_dir.join(&fixture.path);
    let world_json = std::fs::read_to_string(&fixture_path)
        .with_context(|| format!("reading fixture {}", fixture_path.display()))?;
    let gold = Benchmark::load(&world_json, content_hash)
        .with_context(|| {
            format!(
                "loading fixture {} at its pinned hash",
                fixture_path.display()
            )
        })?
        .world;

    // `validate_extractor` above guarantees this is the only reachable strategy.
    let knowledge = Knowledge::open(db)
        .await
        .context("opening the knowledge store")?;
    let strategy = StageReclassifyingExtractor::new(RagSoftmaxClassifier::new(
        KnowledgeRetriever::new(knowledge),
    ));
    let predicted = strategy
        .extract(&gold)
        .await
        .context("running extraction over the fixture skeleton")?;

    let report = AlignmentScorer.score(&gold, &predicted);
    let fingerprint = report_fingerprint(&report);

    eprintln!(
        "eval {work} ({}) via {extractor}: structural P={:.3} R={:.3} F1={:.3}; fingerprint {fingerprint:#018x}",
        fixture.axis, report.structural_precision, report.structural_recall, report.structural_f1
    );

    let output = serde_json::json!({
        "work": work,
        "extractor": extractor,
        "axis": fixture.axis,
        "report": report,
        "report_fingerprint": format!("{fingerprint:#018x}"),
    });
    let serialized =
        serde_json::to_string_pretty(&output).context("serializing the evaluation report")?;
    match out {
        Some(path) => {
            std::fs::write(&path, &serialized)
                .with_context(|| format!("writing report to {}", path.display()))?;
            eprintln!("Report written to {}", path.display());
        }
        None => println!("{serialized}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> BenchmarkIndex {
        serde_json::from_str(
            r#"{ "fixtures": [
                { "id": "odyssey_campbell_macro", "path": "greek/odyssey.json",
                  "axis": "campbell_macro", "content_hash": "0x60d092bed27740f9" }
            ] }"#,
        )
        .expect("test index parses")
    }

    #[test]
    fn parse_content_hash_should_accept_a_0x_prefixed_hex_string() {
        assert_eq!(
            parse_content_hash("0x60d092bed27740f9").expect("valid hex"),
            0x60d0_92be_d277_40f9
        );
    }

    #[test]
    fn parse_content_hash_should_accept_a_bare_hex_string() {
        assert_eq!(parse_content_hash("ff").expect("valid hex"), 255);
    }

    #[test]
    fn parse_content_hash_should_reject_non_hex() {
        assert!(parse_content_hash("0xnothex").is_err());
    }

    #[test]
    fn resolve_fixture_should_find_a_registered_work() {
        let fixture = resolve_fixture(&index(), "odyssey_campbell_macro").expect("registered");
        assert_eq!(fixture.path, "greek/odyssey.json");
        assert_eq!(fixture.axis, "campbell_macro");
    }

    #[test]
    fn resolve_fixture_should_error_on_an_unknown_work() {
        let error = resolve_fixture(&index(), "not_a_work").expect_err("unknown work");
        assert!(error.to_string().contains("odyssey_campbell_macro"));
    }

    #[test]
    fn validate_extractor_should_accept_rag_softmax_and_reject_others() {
        assert!(validate_extractor("rag-softmax").is_ok());
        let error = validate_extractor("llm").expect_err("unknown extractor");
        assert!(error.to_string().contains("rag-softmax"));
    }
}
