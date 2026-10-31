use std::collections::BTreeMap;
use std::path::Path;

use monomyth_frameworks::{LawArtifact, LawError, LawItem, LawSynthesis, load_law};
use monomyth_knowledge::{Namespace, Passage};
use monomyth_synthesis::CandidateLaw;

use super::{
    DraftedLaw, SynthesizePromoteArgs, rejects_forbidden_out_dir, review_required_banner,
    run_synthesize_promote, validate_law_slug, write_candidate,
};

#[test]
fn should_reject_a_law_slug_containing_a_parent_directory_segment() {
    assert!(validate_law_slug("../frameworks/x").is_err());
}

#[test]
fn should_reject_an_absolute_path_law_slug() {
    assert!(validate_law_slug("/abs/x").is_err());
}

#[test]
fn should_reject_a_law_slug_containing_a_path_separator() {
    assert!(validate_law_slug("a/b").is_err());
}

#[test]
fn should_reject_a_law_slug_containing_a_dot() {
    assert!(validate_law_slug("a.b").is_err());
}

#[test]
fn should_reject_an_empty_law_slug() {
    assert!(validate_law_slug("").is_err());
}

#[test]
fn should_reject_a_law_slug_over_the_length_limit() {
    let overlong = "a".repeat(65);
    assert!(validate_law_slug(&overlong).is_err());
}

#[test]
fn should_accept_a_well_formed_law_slug() {
    assert!(validate_law_slug("monomyth_macro_arc").is_ok());
}

#[test]
fn should_refuse_a_path_directly_under_artifacts() {
    let reason = rejects_forbidden_out_dir(Path::new("./artifacts/laws"));
    assert!(reason.is_some(), "./artifacts/laws must be refused");
    assert!(reason.unwrap().contains("synthesis/candidates"));
}

#[test]
fn should_refuse_bare_artifacts_directory() {
    assert!(rejects_forbidden_out_dir(Path::new("artifacts")).is_some());
}

#[test]
fn should_refuse_artifacts_nested_deep_in_the_path() {
    assert!(rejects_forbidden_out_dir(Path::new("foo/artifacts/candidates")).is_some());
}

#[test]
fn should_accept_the_default_candidates_directory() {
    assert!(rejects_forbidden_out_dir(Path::new("./synthesis/candidates")).is_none());
}

#[test]
fn should_accept_an_unrelated_relative_directory() {
    assert!(rejects_forbidden_out_dir(Path::new("./out")).is_none());
}

#[test]
fn should_accept_an_unrelated_absolute_directory() {
    assert!(rejects_forbidden_out_dir(Path::new("/tmp/x")).is_none());
}

/// Build a hand-made [`DraftedLaw`] for offline testing — no LLM, no
/// retrieval. `synthesis.reviewed_by` is left empty, matching what
/// `draft_law` always produces.
fn sample_drafted_law() -> DraftedLaw {
    let items = vec![
        LawItem {
            id: 1,
            name: "Setup".to_owned(),
            description: "The ordinary world before the call.".to_owned(),
            fields: BTreeMap::new(),
        },
        LawItem {
            id: 2,
            name: "Confrontation".to_owned(),
            description: "The central structural conflict.".to_owned(),
            fields: BTreeMap::new(),
        },
    ];
    let artifact = LawArtifact {
        law: "sample_law".to_owned(),
        title: "Sample Law".to_owned(),
        license: "idea/taxonomy only; synthesized structural law".to_owned(),
        namespace: "ship".to_owned(),
        tier: "system".to_owned(),
        domain: "myth".to_owned(),
        tier_note: "test fixture".to_owned(),
        synthesis: LawSynthesis {
            reference_source_ids: vec!["some_reference_source".to_owned()],
            model: "test/stub-model".to_owned(),
            generated: "2026-07-12".to_owned(),
            reviewed_by: String::new(),
            candidate_sha256: Some("deadbeef".to_owned()),
        },
        count: items.len(),
        items,
    };
    let passage = Passage {
        text: "some reference passage text".to_owned(),
        source_id: "some_reference_source".to_owned(),
        score: 0.9,
        namespace: Namespace::Reference,
        license: "in-copyright".to_owned(),
        url: None,
        checksum: None,
        retrieved: None,
    };
    DraftedLaw {
        candidate: CandidateLaw {
            title: "Sample Law".to_owned(),
            items: vec![],
        },
        artifact,
        passages: vec![passage],
        usage: None,
        candidate_sha256: "deadbeef".to_owned(),
        final_score: 90.0,
        iterations: 1,
        verdict: None,
        pre_score: None,
    }
}

#[test]
fn should_write_both_candidate_files_and_round_trip_the_artifact() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let drafted = sample_drafted_law();

    let artifact_path = write_candidate(&drafted, "sample_law", "a sample query", temp_dir.path())
        .expect("writing the candidate succeeds");
    let context_path = temp_dir.path().join("sample_law.context.json");

    assert!(artifact_path.exists(), "artifact file must exist");
    assert!(context_path.exists(), "context file must exist");
    assert_eq!(artifact_path, temp_dir.path().join("sample_law.json"));

    let context_json = std::fs::read_to_string(&context_path).expect("context file reads back");
    assert!(
        context_json.contains("a sample query"),
        "context file must record the grounding query for the reviewer"
    );
    assert!(
        context_json.contains("\"final_score\": 90.0"),
        "context file must record the judge's final score for the reviewer"
    );
    assert!(
        context_json.contains("\"iterations\": 1"),
        "context file must record the judge iteration count for the reviewer"
    );

    let artifact_json = std::fs::read_to_string(&artifact_path).expect("artifact file reads back");
    let round_tripped: LawArtifact =
        serde_json::from_str(&artifact_json).expect("artifact JSON parses");
    assert_eq!(round_tripped.law, "sample_law");
    assert_eq!(round_tripped.count, 2);
    assert_eq!(round_tripped.items.len(), 2);

    // A written candidate must be structurally incapable of shipping: its ~keep
    // empty `reviewed_by` makes `load_law` refuse it outright. ~keep
    let load_result = load_law(&artifact_json);
    assert!(
        matches!(load_result, Err(LawError::MissingReviewer { .. })),
        "an unreviewed candidate must fail load_law with MissingReviewer, got {load_result:?}"
    );
}

#[test]
fn should_reject_writing_a_candidate_with_an_unsafe_law_id() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let drafted = sample_drafted_law();

    let error = write_candidate(&drafted, "../escape", "a sample query", temp_dir.path())
        .expect_err("an unsafe --law id must be rejected before any file is written");

    assert!(
        format!("{error:#}").contains("law id"),
        "error must name the law id validation failure, got: {error:#}"
    );
    // The validation must run before any file is written under `out_dir` — self-contained,
    // unlike probing a path outside `temp_dir` in the shared system temp root. `temp_dir`
    // empty rather than checking for its absence.
    assert_eq!(
        std::fs::read_dir(temp_dir.path())
            .expect("temp_dir reads back")
            .count(),
        0,
        "no file must be written under out_dir when the law id is rejected"
    );
}

#[test]
fn default_config_synthesis_knobs_match_loop_config_default() {
    use monomyth_config::ConfigResolver;
    use monomyth_synthesis::LoopConfig;

    let config = ConfigResolver::defaults()
        .resolve()
        .expect("defaults validate");
    let loop_default = LoopConfig::default();
    assert_eq!(
        *config.synthesis.max_iterations.get(),
        loop_default.max_iterations
    );
    assert_eq!(
        *config.synthesis.per_query_top_k.get(),
        loop_default.per_query_top_k
    );
    assert_eq!(
        *config.synthesis.max_grounding.get(),
        loop_default.max_grounding
    );
}

#[test]
fn banner_text_should_name_the_review_boundary() {
    let banner = review_required_banner(
        "sample_law",
        Path::new("./synthesis/candidates/sample_law.json"),
        Path::new("./synthesis/candidates/sample_law.context.json"),
        82.0,
        2,
    );
    assert!(
        banner.contains("reviewed_by"),
        "banner must mention reviewed_by"
    );
    assert!(
        banner.contains("artifacts/laws"),
        "banner must mention artifacts/laws"
    );
    assert!(banner.contains("REVIEW REQUIRED"));
    assert!(
        banner.contains("Judge score: 82/100 after 2 iteration(s)"),
        "banner must surface the judge score and iteration count"
    );
}

/// Write a minimal fixture candidate law + review-context sidecar into
/// `dir`, returning the candidate's path.
///
/// Deliberately hand-rolled JSON rather than routed through
/// [`write_candidate`]/[`DraftedLaw`]: promotion only cares about the
/// on-disk shapes (a [`LawArtifact`] JSON file plus a `passages`
/// sidecar), not how a real draft run produced them.
fn write_fixture_candidate(
    dir: &Path,
    law_id: &str,
    item_description: &str,
    sidecar_text: &str,
) -> std::path::PathBuf {
    let candidate_json = format!(
        r#"{{
  "law": "{law_id}",
  "title": "Fixture Law",
  "license": "idea/taxonomy only; test fixture",
  "namespace": "ship",
  "tier": "system",
  "domain": "myth",
  "tier_note": "test fixture",
  "synthesis": {{
    "reference_source_ids": ["fixture_source"],
    "model": "test/stub-model",
    "generated": "2026-07-21",
    "reviewed_by": "",
    "candidate_sha256": "deadbeef"
  }},
  "count": 1,
  "items": [
    {{ "id": 1, "name": "Setup", "description": {item_description:?} }}
  ]
}}"#
    );
    let candidate_path = dir.join(format!("{law_id}.json"));
    std::fs::write(&candidate_path, candidate_json).expect("fixture candidate writes");

    let sidecar_json =
        format!(r#"{{"passages": [{{"source_id": "fixture_source", "text": {sidecar_text:?}}}]}}"#);
    let sidecar_path = dir.join(format!("{law_id}.context.json"));
    std::fs::write(&sidecar_path, sidecar_json).expect("fixture sidecar writes");

    candidate_path
}

/// Write a fixture candidate law + review-context sidecar into `dir` with
/// full control over `title`, `tier_note`, and the sidecar's passages —
/// needed to exercise the anti-leak gate against fields
/// [`write_fixture_candidate`] hardcodes.
fn write_fixture_candidate_full(
    dir: &Path,
    law_id: &str,
    title: &str,
    tier_note: &str,
    item_description: &str,
    sidecar_passages: &[(&str, &str)],
) -> std::path::PathBuf {
    let candidate_json = format!(
        r#"{{
  "law": "{law_id}",
  "title": {title:?},
  "license": "idea/taxonomy only; test fixture",
  "namespace": "ship",
  "tier": "system",
  "domain": "myth",
  "tier_note": {tier_note:?},
  "synthesis": {{
    "reference_source_ids": ["fixture_source"],
    "model": "test/stub-model",
    "generated": "2026-07-21",
    "reviewed_by": "",
    "candidate_sha256": "deadbeef"
  }},
  "count": 1,
  "items": [
    {{ "id": 1, "name": "Setup", "description": {item_description:?} }}
  ]
}}"#
    );
    let candidate_path = dir.join(format!("{law_id}.json"));
    std::fs::write(&candidate_path, candidate_json).expect("fixture candidate writes");

    let passages_json: Vec<String> = sidecar_passages
        .iter()
        .map(|(source_id, text)| format!(r#"{{"source_id": {source_id:?}, "text": {text:?}}}"#))
        .collect();
    let sidecar_json = format!(r#"{{"passages": [{}]}}"#, passages_json.join(", "));
    let sidecar_path = dir.join(format!("{law_id}.context.json"));
    std::fs::write(&sidecar_path, sidecar_json).expect("fixture sidecar writes");

    candidate_path
}

/// Write a minimal `index.json` fixture (empty `laws` array) into `laws_dir`.
fn write_fixture_index(laws_dir: &Path) {
    std::fs::create_dir_all(laws_dir).expect("laws dir creates");
    std::fs::write(
        laws_dir.join("index.json"),
        r#"{"index": "test index", "note": "test note", "laws": []}"#,
    )
    .expect("index fixture writes");
}

#[test]
fn should_reject_promotion_with_an_empty_reviewed_by_before_touching_the_filesystem() {
    let error = run_synthesize_promote(SynthesizePromoteArgs {
        candidate: std::path::PathBuf::from("/nonexistent/candidate.json"),
        reviewed_by: String::new(),
        laws_dir: std::path::PathBuf::from("/nonexistent/laws"),
    })
    .expect_err("an empty reviewer must be rejected before any file IO");
    assert!(error.to_string().contains("--reviewed-by"));
}

#[test]
fn should_reject_promotion_with_a_whitespace_only_reviewed_by() {
    let error = run_synthesize_promote(SynthesizePromoteArgs {
        candidate: std::path::PathBuf::from("/nonexistent/candidate.json"),
        reviewed_by: "   ".to_owned(),
        laws_dir: std::path::PathBuf::from("/nonexistent/laws"),
    })
    .expect_err("a whitespace-only reviewer must be rejected before any file IO");
    assert!(error.to_string().contains("--reviewed-by"));
}

#[test]
fn should_promote_a_fixture_candidate_and_update_the_index() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let candidate_path = write_fixture_candidate(
        temp_dir.path(),
        "fixture_law",
        "A harmless made-up description with no overlap.",
        "some unrelated reference passage text about a wholly different topic entirely",
    );
    let laws_dir = temp_dir.path().join("laws");
    write_fixture_index(&laws_dir);

    run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path,
        reviewed_by: "alice".to_owned(),
        laws_dir: laws_dir.clone(),
    })
    .expect("promotion of a clean fixture candidate succeeds");

    let law_path = laws_dir.join("fixture_law.json");
    assert!(law_path.exists(), "promoted law file must exist");

    let law_json = std::fs::read_to_string(&law_path).expect("law file reads back");
    let loaded = load_law(&law_json).expect("promoted law must load_law cleanly");
    assert_eq!(loaded.law, "fixture_law");
    assert_eq!(loaded.synthesis.reviewed_by, "alice");
    assert_eq!(
        loaded.synthesis.candidate_sha256.as_deref(),
        Some("deadbeef"),
        "promotion must not recompute candidate_sha256"
    );

    let index_json =
        std::fs::read_to_string(laws_dir.join("index.json")).expect("index reads back");
    let index: serde_json::Value = serde_json::from_str(&index_json).expect("index is valid JSON");
    let laws = index["laws"].as_array().expect("laws array");
    assert!(
        laws.iter()
            .any(|entry| entry["law"] == "fixture_law" && entry["count"] == 1),
        "index must gain an entry for the promoted law"
    );
}

#[test]
fn should_persist_a_trimmed_reviewed_by() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let candidate_path = write_fixture_candidate(
        temp_dir.path(),
        "trimmed_reviewer_law",
        "A harmless made-up description with no overlap.",
        "some unrelated reference passage text about a wholly different topic entirely",
    );
    let laws_dir = temp_dir.path().join("laws");
    write_fixture_index(&laws_dir);

    run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path,
        reviewed_by: "  alice  ".to_owned(),
        laws_dir: laws_dir.clone(),
    })
    .expect("promotion with a padded reviewer identity succeeds");

    let law_json = std::fs::read_to_string(laws_dir.join("trimmed_reviewer_law.json"))
        .expect("law file reads back");
    let loaded = load_law(&law_json).expect("promoted law must load_law cleanly");
    assert_eq!(
        loaded.synthesis.reviewed_by, "alice",
        "the persisted reviewer identity must be trimmed of surrounding whitespace"
    );
}

#[test]
fn should_reject_promotion_when_item_text_verbatim_overlaps_the_sidecar() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let overlapping_description =
        "the suppliant implores a power in authority to grant a boon of mercy";
    let candidate_path = write_fixture_candidate(
        temp_dir.path(),
        "leaky_law",
        overlapping_description,
        "The Suppliant implores a Power in authority to grant a boon of mercy.",
    );
    let laws_dir = temp_dir.path().join("laws");
    write_fixture_index(&laws_dir);

    let error = run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path,
        reviewed_by: "alice".to_owned(),
        laws_dir: laws_dir.clone(),
    })
    .expect_err("a verbatim 8-word overlap must be rejected on re-check");

    assert!(
        format!("{error:#}").to_lowercase().contains("anti-leak"),
        "error must name the anti-leak re-check, got: {error:#}"
    );
    assert!(
        !laws_dir.join("leaky_law.json").exists(),
        "a rejected candidate must not be written into laws_dir"
    );
}

#[test]
fn should_reject_promotion_when_title_verbatim_overlaps_the_sidecar() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let overlapping_title = "The Suppliant implores a Power in authority to grant a boon";
    let candidate_path = write_fixture_candidate_full(
        temp_dir.path(),
        "leaky_title_law",
        overlapping_title,
        "test fixture",
        "A harmless made-up description with no overlap at all here.",
        &[(
            "fixture_source",
            "The Suppliant implores a Power in authority to grant a boon of mercy.",
        )],
    );
    let laws_dir = temp_dir.path().join("laws");
    write_fixture_index(&laws_dir);

    let error = run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path,
        reviewed_by: "alice".to_owned(),
        laws_dir: laws_dir.clone(),
    })
    .expect_err("a verbatim overlap in the artifact title must be rejected on re-check");

    assert!(
        format!("{error:#}").to_lowercase().contains("anti-leak"),
        "error must name the anti-leak re-check, got: {error:#}"
    );
    assert!(!laws_dir.join("leaky_title_law.json").exists());
}

#[test]
fn should_reject_promotion_when_tier_note_verbatim_overlaps_the_sidecar() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let overlapping_tier_note = "the suppliant implores a power in authority to grant a boon";
    let candidate_path = write_fixture_candidate_full(
        temp_dir.path(),
        "leaky_tier_note_law",
        "Fixture Law",
        overlapping_tier_note,
        "A harmless made-up description with no overlap at all here.",
        &[(
            "fixture_source",
            "The Suppliant implores a Power in authority to grant a boon of mercy.",
        )],
    );
    let laws_dir = temp_dir.path().join("laws");
    write_fixture_index(&laws_dir);

    let error = run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path,
        reviewed_by: "alice".to_owned(),
        laws_dir: laws_dir.clone(),
    })
    .expect_err("a verbatim overlap in the artifact tier_note must be rejected on re-check");

    assert!(
        format!("{error:#}").to_lowercase().contains("anti-leak"),
        "error must name the anti-leak re-check, got: {error:#}"
    );
    assert!(!laws_dir.join("leaky_tier_note_law.json").exists());
}

#[test]
fn should_reject_promotion_when_sidecar_records_no_grounding_passages() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let candidate_path = write_fixture_candidate_full(
        temp_dir.path(),
        "unguarded_law",
        "Fixture Law",
        "test fixture",
        "A harmless made-up description with no overlap at all here.",
        &[],
    );
    let laws_dir = temp_dir.path().join("laws");
    write_fixture_index(&laws_dir);

    let error = run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path,
        reviewed_by: "alice".to_owned(),
        laws_dir: laws_dir.clone(),
    })
    .expect_err("an empty sidecar passages array must not silently disable the anti-leak gate");

    assert!(
        error.to_string().to_lowercase().contains("grounding"),
        "error must explain the sidecar records no grounding passages, got: {error}"
    );
    assert!(!laws_dir.join("unguarded_law.json").exists());
}

#[test]
fn should_reject_promotion_when_every_sidecar_passage_is_too_short_to_shingle() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    // Present, non-empty passages, but each is under the anti-leak gate's shingle
    // width: they yield zero shingles, so the re-check would silently pass anything
    // — exactly as neutered as an empty passages array.
    let candidate_path = write_fixture_candidate_full(
        temp_dir.path(),
        "short_passage_law",
        "Fixture Law",
        "test fixture",
        "A harmless made-up description with no overlap at all here.",
        &[("fixture_source", "too short"), ("other_source", "")],
    );
    let laws_dir = temp_dir.path().join("laws");
    write_fixture_index(&laws_dir);

    let error = run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path,
        reviewed_by: "alice".to_owned(),
        laws_dir: laws_dir.clone(),
    })
    .expect_err(
        "passages present but all too short to shingle must not silently disable the \
             anti-leak gate",
    );

    assert!(
        error.to_string().to_lowercase().contains("grounding"),
        "error must explain the sidecar records no usable grounding passages, got: {error}"
    );
    assert!(!laws_dir.join("short_passage_law.json").exists());
}

#[test]
fn should_promote_the_first_law_into_a_fresh_laws_directory() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let candidate_path = write_fixture_candidate(
        temp_dir.path(),
        "bootstrap_law",
        "A harmless made-up description with no overlap at all here.",
        "some unrelated reference passage text about a wholly different topic entirely",
    );
    // Deliberately does not exist yet, and no index.json fixture is written: the
    // very first `synthesize promote` into a brand-new repo must bootstrap both.
    let laws_dir = temp_dir.path().join("laws");

    run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path,
        reviewed_by: "alice".to_owned(),
        laws_dir: laws_dir.clone(),
    })
    .expect("promoting the first law into a fresh laws directory succeeds");

    assert!(laws_dir.join("bootstrap_law.json").exists());
    let index_json =
        std::fs::read_to_string(laws_dir.join("index.json")).expect("index reads back");
    let index: serde_json::Value = serde_json::from_str(&index_json).expect("index is valid JSON");
    assert!(
        index["laws"]
            .as_array()
            .expect("laws array")
            .iter()
            .any(|entry| entry["law"] == "bootstrap_law"),
        "index must gain an entry for the promoted law"
    );
}

#[test]
fn should_recover_from_an_unindexed_orphan_law_file() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let candidate_path = write_fixture_candidate(
        temp_dir.path(),
        "orphan_law",
        "A harmless made-up description with no overlap at all here.",
        "some unrelated reference passage text about a wholly different topic entirely",
    );
    let laws_dir = temp_dir.path().join("laws");
    write_fixture_index(&laws_dir);
    // Simulate an interrupted prior promotion: the law file landed on disk but the
    // process crashed before the index write registered it.
    std::fs::write(
        laws_dir.join("orphan_law.json"),
        "stale content from a crashed run",
    )
    .expect("orphan fixture writes");

    run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path,
        reviewed_by: "alice".to_owned(),
        laws_dir: laws_dir.clone(),
    })
    .expect("an unindexed orphan law file must not permanently block re-promotion");

    let law_json =
        std::fs::read_to_string(laws_dir.join("orphan_law.json")).expect("law file reads back");
    let loaded = load_law(&law_json).expect("the recovered promotion must load_law cleanly");
    assert_eq!(loaded.synthesis.reviewed_by, "alice");
}

#[test]
fn should_refuse_to_promote_an_already_promoted_law() {
    let temp_dir = tempfile::tempdir().expect("tempdir creates");
    let candidate_path = write_fixture_candidate(
        temp_dir.path(),
        "dup_law",
        "A harmless made-up description with no overlap at all.",
        "some unrelated reference passage text about a wholly different subject entirely",
    );
    let laws_dir = temp_dir.path().join("laws");
    write_fixture_index(&laws_dir);

    run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path.clone(),
        reviewed_by: "alice".to_owned(),
        laws_dir: laws_dir.clone(),
    })
    .expect("first promotion succeeds");

    let error = run_synthesize_promote(SynthesizePromoteArgs {
        candidate: candidate_path,
        reviewed_by: "bob".to_owned(),
        laws_dir,
    })
    .expect_err("a second promotion of the same law must be refused");

    assert!(
        error.to_string().contains("already"),
        "error must explain the law is already promoted, got: {error}"
    );
}
