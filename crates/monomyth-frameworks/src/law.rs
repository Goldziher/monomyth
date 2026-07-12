//! Generic runtime loader for law artifacts (`artifacts/laws/*.json`, ADR-0016).
//!
//! Unlike the base taxonomies in [`crate::info`], laws have no fixed vocabulary:
//! they are synthesized over time by a human-reviewed build-time pipeline, so
//! there is no `framework_enum!` to generate ahead of time. [`load_law`] parses
//! and validates a law artifact from a `&str` at call time instead, returning a
//! [`LawArtifact`] value that carries its own items rather than indexing into a
//! `LazyLock`.
//!
//! Every law is stamped `tier: "system"` (an uncopyrightable idea/taxonomy — no
//! source prose reproduced) and `namespace: "ship"` (shippable), and its
//! [`LawSynthesis::reviewed_by`] field records the human reviewer who approved it
//! before commit. [`load_law`] enforces both invariants, plus structural
//! well-formedness (`count` matching the item list, contiguous item ids).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The licensing tier every law artifact must declare.
const REQUIRED_TIER: &str = "system";

/// The shippability namespace every law artifact must declare.
const REQUIRED_NAMESPACE: &str = "ship";

/// Provenance of the build-time synthesis step that produced a [`LawArtifact`]
/// (ADR-0016: map -> synthesize -> configure).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LawSynthesis {
    /// Ids of `reference`-namespace `corpus/manifest.json` sources this law was
    /// synthesized from. Empty for a hand-authored law such as the loader fixture.
    pub reference_source_ids: Vec<String>,
    /// The model (or `"(authored)"`) that produced the synthesis candidate.
    pub model: String,
    /// Date the synthesis candidate was generated (ISO 8601 date).
    pub generated: String,
    /// The human reviewer who approved this law before commit. Mandatory and
    /// validated non-empty by [`load_law`].
    pub reviewed_by: String,
    /// Checksum of the pre-review synthesis candidate, for audit trail.
    #[serde(default)]
    pub candidate_sha256: Option<String>,
}

/// One item of a [`LawArtifact`].
///
/// Only `id`, `name`, and `description` are common across every law; any
/// law-specific properties (e.g. an ordered `phase_order` list) are captured in
/// [`fields`](Self::fields) via `#[serde(flatten)]` rather than assumed ahead of
/// time, since laws have no fixed vocabulary.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LawItem {
    /// Stable 1-based item id, contiguous within the artifact.
    pub id: u16,
    /// Canonical item name.
    pub name: String,
    /// Authored description of the item.
    pub description: String,
    /// Law-specific properties beyond `id`/`name`/`description`.
    #[serde(flatten)]
    pub fields: BTreeMap<String, serde_json::Value>,
}

/// A build-time-synthesized, human-reviewed structural law (ADR-0016).
///
/// Parsed and validated at runtime from a law artifact's JSON text by
/// [`load_law`], rather than embedded as a compile-time enum, since the law
/// vocabulary grows over time as new laws are synthesized and reviewed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LawArtifact {
    /// Stable machine-readable law identifier.
    pub law: String,
    /// Human-readable title of the law artifact.
    pub title: String,
    /// Licensing note for this artifact's content.
    pub license: String,
    /// Shippability namespace. Always `"ship"` in a valid artifact.
    pub namespace: String,
    /// Licensing tier. Always `"system"` in a valid artifact.
    pub tier: String,
    /// The corpus domain this law belongs to (e.g. `myth`, `folklore`).
    pub domain: String,
    /// Human-readable note explaining why this artifact is ship-safe.
    #[serde(default)]
    pub tier_note: String,
    /// Provenance of the synthesis step that produced this law.
    pub synthesis: LawSynthesis,
    /// Number of items; must equal `items.len()` in a valid artifact.
    pub count: usize,
    /// The law's ordered, contiguously-id'd items.
    pub items: Vec<LawItem>,
}

/// Why a law artifact failed to load.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LawError {
    /// The input was not schema-valid JSON for [`LawArtifact`].
    #[error("law artifact is not valid JSON: {0}")]
    Parse(#[from] serde_json::Error),
    /// The artifact does not declare `tier: "system"` and `namespace: "ship"`,
    /// the only licensing state a law artifact may carry (ADR-0016).
    #[error(
        "law {law:?} violates the licensing invariant: tier must be \"system\" and namespace \
         must be \"ship\", found tier={tier:?} namespace={namespace:?}"
    )]
    LicensingInvariant {
        /// The offending law's identifier.
        law: String,
        /// The actual `tier` value found.
        tier: String,
        /// The actual `namespace` value found.
        namespace: String,
    },
    /// The declared `count` does not match the number of parsed `items`.
    #[error("law {law:?} declares count={count} but has {items} items")]
    CountMismatch {
        /// The offending law's identifier.
        law: String,
        /// The declared `count` field.
        count: usize,
        /// The actual number of parsed items.
        items: usize,
    },
    /// Item ids are not the contiguous sequence `1..=count` in order.
    #[error("law {law:?} item ids are not contiguous 1..=count in order")]
    NonContiguousIds {
        /// The offending law's identifier.
        law: String,
    },
    /// `synthesis.reviewed_by` is empty (or all whitespace).
    ///
    /// ADR-0016 requires human review of every synthesized law before commit;
    /// an empty reviewer defeats that guarantee.
    #[error("law {law:?} is missing a synthesis.reviewed_by reviewer")]
    MissingReviewer {
        /// The offending law's identifier.
        law: String,
    },
}

/// Parse and validate a law artifact from its JSON text.
///
/// Validation, beyond schema-valid JSON, enforces:
/// - the licensing invariant (`tier == "system"`, `namespace == "ship"`);
/// - `count` matches the number of `items`;
/// - item ids are the contiguous sequence `1..=count` in order;
/// - `synthesis.reviewed_by` is non-empty (ADR-0016 mandates human review).
///
/// # Errors
///
/// Returns [`LawError::Parse`] if `json` is not valid [`LawArtifact`] JSON,
/// [`LawError::LicensingInvariant`] if the tier/namespace pair is not
/// `("system", "ship")`, [`LawError::CountMismatch`] if `count` disagrees with
/// the item list length, [`LawError::NonContiguousIds`] if item ids are not
/// `1..=count` in order, or [`LawError::MissingReviewer`] if
/// `synthesis.reviewed_by` is empty or whitespace-only.
///
/// ```
/// # use monomyth_frameworks::load_law;
/// const EXAMPLE: &str = include_str!("../../../artifacts/laws/example_two_arc.json");
/// let law = load_law(EXAMPLE)?;
/// assert_eq!(law.law, "example_two_arc");
/// # Ok::<(), monomyth_frameworks::LawError>(())
/// ```
pub fn load_law(json: &str) -> Result<LawArtifact, LawError> {
    let artifact: LawArtifact = serde_json::from_str(json)?;
    validate_licensing_invariant(&artifact)?;
    validate_count(&artifact)?;
    validate_contiguous_ids(&artifact)?;
    validate_reviewer(&artifact)?;
    Ok(artifact)
}

/// Enforce `tier == "system"` and `namespace == "ship"`.
fn validate_licensing_invariant(artifact: &LawArtifact) -> Result<(), LawError> {
    if artifact.tier == REQUIRED_TIER && artifact.namespace == REQUIRED_NAMESPACE {
        return Ok(());
    }
    Err(LawError::LicensingInvariant {
        law: artifact.law.clone(),
        tier: artifact.tier.clone(),
        namespace: artifact.namespace.clone(),
    })
}

/// Enforce `count == items.len()`.
fn validate_count(artifact: &LawArtifact) -> Result<(), LawError> {
    if artifact.count == artifact.items.len() {
        return Ok(());
    }
    Err(LawError::CountMismatch {
        law: artifact.law.clone(),
        count: artifact.count,
        items: artifact.items.len(),
    })
}

/// Enforce that item ids are the contiguous sequence `1..=count` in order.
fn validate_contiguous_ids(artifact: &LawArtifact) -> Result<(), LawError> {
    for (index, item) in artifact.items.iter().enumerate() {
        let expected = u16::try_from(index + 1).unwrap_or(u16::MAX);
        if item.id != expected {
            return Err(LawError::NonContiguousIds {
                law: artifact.law.clone(),
            });
        }
    }
    Ok(())
}

/// Enforce that `synthesis.reviewed_by` is non-empty after trimming.
fn validate_reviewer(artifact: &LawArtifact) -> Result<(), LawError> {
    if !artifact.synthesis.reviewed_by.trim().is_empty() {
        return Ok(());
    }
    Err(LawError::MissingReviewer {
        law: artifact.law.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::{LawError, load_law};

    /// The self-authored loader fixture: not synthesized from any source, so its
    /// `synthesis.reference_source_ids` is empty and the reference-source
    /// resolution test below is vacuous for it.
    const EXAMPLE: &str = include_str!("../../../artifacts/laws/example_two_arc.json");

    /// The registry mirroring `artifacts/frameworks/index.json`'s shape.
    const INDEX: &str = include_str!("../../../artifacts/laws/index.json");

    /// The corpus source ledger, read as raw JSON so this pure crate does not
    /// depend on `monomyth-knowledge`.
    const MANIFEST: &str = include_str!("../../../corpus/manifest.json");

    #[test]
    fn load_law_accepts_the_example_fixture() {
        let law = load_law(EXAMPLE).expect("example fixture is a valid law artifact");

        assert_eq!(law.law, "example_two_arc");
        assert_eq!(law.tier, "system");
        assert_eq!(law.namespace, "ship");
        assert_eq!(law.count, 2);
        assert_eq!(law.items.len(), 2);
        assert_eq!(
            law.items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert!(!law.synthesis.reviewed_by.trim().is_empty());
        assert!(
            law.items[0].fields.contains_key("phase_order"),
            "law-specific fields must be captured via #[serde(flatten)]"
        );
    }

    /// Parse [`EXAMPLE`] into a mutable [`serde_json::Value`], apply `mutate`, and
    /// re-serialize it back into JSON text for a negative [`load_law`] test.
    fn mutate_example(mutate: impl FnOnce(&mut serde_json::Value)) -> String {
        let mut value: serde_json::Value =
            serde_json::from_str(EXAMPLE).expect("example fixture is valid JSON");
        mutate(&mut value);
        serde_json::to_string(&value).expect("mutated value serializes")
    }

    #[test]
    fn load_law_rejects_a_non_system_tier() {
        let json = mutate_example(|value| {
            value["tier"] = serde_json::Value::String("reference".to_owned());
        });

        let error = load_law(&json).expect_err("reference tier violates the licensing invariant");

        match error {
            LawError::LicensingInvariant {
                law,
                tier,
                namespace,
            } => {
                assert_eq!(law, "example_two_arc");
                assert_eq!(tier, "reference");
                assert_eq!(namespace, "ship");
            }
            other => panic!("expected LicensingInvariant, got {other:?}"),
        }
    }

    #[test]
    fn load_law_rejects_a_count_that_disagrees_with_items() {
        let json = mutate_example(|value| {
            value["items"]
                .as_array_mut()
                .expect("items is an array")
                .pop();
        });

        let error = load_law(&json).expect_err("count must match items.len()");

        match error {
            LawError::CountMismatch { law, count, items } => {
                assert_eq!(law, "example_two_arc");
                assert_eq!(count, 2);
                assert_eq!(items, 1);
            }
            other => panic!("expected CountMismatch, got {other:?}"),
        }
    }

    #[test]
    fn load_law_rejects_a_blank_reviewer() {
        let json = mutate_example(|value| {
            value["synthesis"]["reviewed_by"] = serde_json::Value::String("   ".to_owned());
        });

        let error = load_law(&json).expect_err("whitespace-only reviewer must be rejected");

        match error {
            LawError::MissingReviewer { law } => assert_eq!(law, "example_two_arc"),
            other => panic!("expected MissingReviewer, got {other:?}"),
        }
    }

    #[test]
    fn load_law_rejects_non_contiguous_item_ids() {
        let json = mutate_example(|value| {
            let items = value["items"].as_array_mut().expect("items is an array");
            items[0]["id"] = serde_json::Value::from(2);
            items[1]["id"] = serde_json::Value::from(1);
        });

        let error = load_law(&json).expect_err("swapped ids are not contiguous in order");

        match error {
            LawError::NonContiguousIds { law } => assert_eq!(law, "example_two_arc"),
            other => panic!("expected NonContiguousIds, got {other:?}"),
        }
    }

    #[test]
    fn index_lists_the_example_law() {
        let value: serde_json::Value = serde_json::from_str(INDEX).expect("index is valid JSON");
        let laws = value["laws"].as_array().expect("laws array");

        let example = laws
            .iter()
            .find(|entry| entry["law"] == "example_two_arc")
            .expect("index lists example_two_arc");

        assert_eq!(example["count"], 2);
        assert_eq!(example["namespace"], "ship");
        assert_eq!(example["tier"], "system");
    }

    /// Assert every id in `reference_source_ids` resolves to a `corpus/manifest.json`
    /// source whose `namespace` is `"reference"`.
    ///
    /// For [`EXAMPLE`] this list is empty, so the assertion is vacuously true; the
    /// check exists so future synthesized laws are verified automatically as soon
    /// as they declare real reference source ids.
    fn assert_reference_sources_resolve(reference_source_ids: &[String]) {
        let manifest: serde_json::Value =
            serde_json::from_str(MANIFEST).expect("corpus manifest is valid JSON");
        let sources = manifest["sources"].as_array().expect("sources array");

        for source_id in reference_source_ids {
            let source = sources
                .iter()
                .find(|entry| entry["id"] == source_id.as_str())
                .unwrap_or_else(|| panic!("reference_source_id {source_id:?} not in manifest"));
            assert_eq!(
                source["namespace"], "reference",
                "reference_source_id {source_id:?} must name a reference-namespace source",
            );
        }
    }

    #[test]
    fn reference_source_ids_resolve_to_reference_namespace_manifest_entries() {
        let law = load_law(EXAMPLE).expect("example fixture is a valid law artifact");
        assert!(
            law.synthesis.reference_source_ids.is_empty(),
            "example fixture is authored, not synthesized, so this check is vacuous for it"
        );
        assert_reference_sources_resolve(&law.synthesis.reference_source_ids);
    }
}
