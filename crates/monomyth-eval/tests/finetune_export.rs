//! Fine-tune export over the committed Odyssey fixture (ADR-0023 B6).
//!
//! Confirms the export produces one prose-free training pair per spine node with
//! the gold scored distribution intact — in particular the `GoddessAndTemptress`
//! node's `TheMeetingWithTheGoddess` / `WomanAsTemptress` split, the fixture's
//! one deliberately-scored classification.

use monomyth_core::World;
use monomyth_eval::{Benchmark, stage_training_examples, to_jsonl};

/// Mirrors `odyssey_fixture.rs`'s `ODYSSEY_CONTENT_HASH`.
const ODYSSEY_CONTENT_HASH: u64 = 0x60d0_92be_d277_40f9;

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

#[test]
fn export_should_produce_one_prose_free_example_per_spine_node() {
    let world = load_odyssey();
    let spine_len = world.story.structure.spine().len();

    let examples = stage_training_examples(&world, "campbell_macro");

    assert_eq!(
        examples.len(),
        spine_len,
        "one training example per spine node"
    );
    for example in &examples {
        assert_eq!(example.axis, "campbell_macro");
        assert!(!example.text.is_empty(), "the hint text must be populated");
        // The primary label always carries a permille entry in the distribution.
        assert_eq!(
            example.distribution.get(&example.primary).copied(),
            Some(1000),
            "the primary must carry Weight::FULL in the distribution"
        );
    }
}

#[test]
fn export_should_preserve_the_goddess_temptress_scored_split() {
    let world = load_odyssey();
    let examples = stage_training_examples(&world, "campbell_macro");

    let split = examples
        .iter()
        .find(|example| example.primary == "The Meeting with the Goddess")
        .expect("the fixture has a Meeting-with-the-Goddess primary node");

    // primary = 1000 (FULL), alternative WomanAsTemptress = 350 permille.
    assert_eq!(
        split.distribution.get("The Meeting with the Goddess"),
        Some(&1000)
    );
    assert_eq!(split.distribution.get("Woman as Temptress"), Some(&350));
}

#[test]
fn jsonl_should_be_one_valid_json_object_per_line() {
    let world = load_odyssey();
    let examples = stage_training_examples(&world, "campbell_macro");

    let jsonl = to_jsonl(&examples).expect("training examples serialize");
    let lines: Vec<&str> = jsonl.lines().collect();

    assert_eq!(lines.len(), examples.len(), "one line per example");
    for line in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).expect("each JSONL line is a valid JSON object");
        assert!(parsed.get("text").is_some());
        assert!(parsed.get("primary").is_some());
        assert!(parsed.get("distribution").is_some());
    }
}
