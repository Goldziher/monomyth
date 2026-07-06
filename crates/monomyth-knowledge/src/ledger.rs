//! The license ledger: the embedded corpus manifest that declares every source,
//! its namespace, tier, license, and domain.
//!
//! The ledger is the single source of truth for the hard commercial invariant: a
//! source tagged `reference` / `noncommercial` / `copyright` may never be surfaced
//! verbatim. Every ingest decision is gated by looking the source up here.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::KnowledgeError;

/// The embedded corpus manifest (`corpus/manifest.json`), baked into the binary
/// so the ledger travels with the shipped product and cannot drift from it.
const MANIFEST_JSON: &str = include_str!("../../../corpus/manifest.json");

/// Collection holding ship-safe material that may be surfaced verbatim.
pub const SHIP_COLLECTION: &str = "ship";
/// Collection holding reference-only material (priors only; never surfaced).
pub const REFERENCE_COLLECTION: &str = "reference";

/// Which of the two trust domains a source belongs to.
///
/// The value is copied onto every stored document's metadata and re-checked at
/// retrieval time, so it is the load-bearing tag for the licensing invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Namespace {
    /// PD / CC0 / CC-BY / CC-BY-SA — may be surfaced verbatim in generated output.
    Ship,
    /// Copyrighted or `NonCommercial` — informs generation only; never redistributed.
    Reference,
}

impl Namespace {
    /// The collection a source in this namespace is stored in.
    #[must_use]
    pub const fn collection(self) -> &'static str {
        match self {
            Namespace::Ship => SHIP_COLLECTION,
            Namespace::Reference => REFERENCE_COLLECTION,
        }
    }

    /// The token this namespace serializes to on the wire and in stored document
    /// metadata. The single source of truth for the `doc.metadata.namespace`
    /// filter value, so the enforcement filter can never drift from the tag that
    /// ingest actually writes (pinned by `wire_token_matches_serde`).
    #[must_use]
    pub const fn as_wire(self) -> &'static str {
        match self {
            Namespace::Ship => "ship",
            Namespace::Reference => "reference",
        }
    }
}

/// The license tier of a source, mirroring `tier_meaning` in the manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Uncopyrightable idea/taxonomy independently encoded — ship-safe, no prose reproduced.
    System,
    /// Public domain — ship-safe, may be surfaced verbatim.
    PublicDomain,
    /// CC0 — ship-safe, no obligations.
    Cc0,
    /// CC-BY / MIT / Apache-2.0 — ship-safe with attribution.
    Permissive,
    /// CC-BY-SA — ship-safe but isolated so copyleft cannot contaminate PD/CC0.
    // The manifest spells this `sharealike` (no underscore); snake_case would emit
    // `share_alike`, so the wire token is pinned explicitly.
    #[serde(rename = "sharealike")]
    ShareAlike,
    /// CC-BY-NC* — reference-only (this is a commercial product).
    Noncommercial,
    /// In-copyright — reference-only, never redistributed.
    Copyright,
    /// Research/mixed-license — reference-only.
    Reference,
}

/// One declared corpus source.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SourceEntry {
    /// Stable identifier used to reference the source at ingest time.
    pub id: String,
    /// Human-readable name.
    pub name: String,
    /// License tier.
    pub tier: Tier,
    /// Trust domain (ship vs reference).
    pub namespace: Namespace,
    /// Content domain (`framework`, `myth`, `folklore`, …).
    pub domain: String,
    /// Free-form license string as declared in the manifest.
    pub license: String,
    /// Optional provenance / handling note.
    #[serde(default)]
    pub note: Option<String>,
    /// Optional source URL.
    #[serde(default)]
    pub url: Option<String>,
}

/// Deserialization shim matching the top-level manifest shape. Unknown fields
/// (`version`, `tier_meaning`, `domains`, …) are ignored.
#[derive(Debug, Deserialize)]
struct Manifest {
    sources: Vec<SourceEntry>,
}

/// The license ledger: every declared source, indexed by id.
#[derive(Debug, Clone)]
pub struct Ledger {
    sources: BTreeMap<String, SourceEntry>,
}

impl Ledger {
    /// Load the ledger from the manifest embedded at compile time.
    ///
    /// # Errors
    ///
    /// Returns [`KnowledgeError::Manifest`] if the embedded manifest cannot be
    /// parsed.
    ///
    /// # Examples
    ///
    /// ```
    /// use monomyth_knowledge::Ledger;
    ///
    /// let ledger = Ledger::load_embedded()?;
    /// assert!(ledger.get("polti").is_some());
    /// # Ok::<(), monomyth_knowledge::KnowledgeError>(())
    /// ```
    pub fn load_embedded() -> Result<Self, KnowledgeError> {
        let manifest: Manifest = serde_json::from_str(MANIFEST_JSON)?;
        let sources = manifest
            .sources
            .into_iter()
            .map(|entry| (entry.id.clone(), entry))
            .collect();
        Ok(Self { sources })
    }

    /// Look up a source by id.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&SourceEntry> {
        self.sources.get(id)
    }

    /// Iterate over all declared sources in deterministic id order.
    pub fn entries(&self) -> impl Iterator<Item = &SourceEntry> {
        self.sources.values()
    }

    /// Number of declared sources.
    #[must_use]
    pub fn len(&self) -> usize {
        self.sources.len()
    }

    /// Whether the ledger declares no sources.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sources.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_load_embedded_ledger_with_sources() {
        let ledger = Ledger::load_embedded().expect("embedded manifest parses");
        assert!(
            !ledger.is_empty(),
            "ledger must declare at least one source"
        );
        assert!(ledger.len() > 20, "manifest declares more than 20 sources");
    }

    #[test]
    fn should_map_namespace_to_collection() {
        assert_eq!(Namespace::Ship.collection(), SHIP_COLLECTION);
        assert_eq!(Namespace::Reference.collection(), REFERENCE_COLLECTION);
    }

    #[test]
    fn wire_token_matches_serde() {
        // The ship filter is built from `as_wire`; ingest writes the serde form.
        // If these ever drift, ship-gated retrieval would filter on a stale token.
        for namespace in [Namespace::Ship, Namespace::Reference] {
            let serialized = serde_json::to_value(namespace).expect("namespace serializes");
            assert_eq!(
                serialized,
                serde_json::Value::String(namespace.as_wire().to_owned()),
                "as_wire must equal the serde representation for {namespace:?}",
            );
        }
    }

    #[test]
    fn should_parse_sharealike_tier_token() {
        let ledger = Ledger::load_embedded().expect("embedded manifest parses");
        let trilogy = ledger.get("trilogy").expect("trilogy is declared");
        assert_eq!(trilogy.tier, Tier::ShareAlike);
        assert_eq!(trilogy.namespace, Namespace::Ship);
    }

    #[test]
    fn every_ship_namespace_source_has_a_ship_safe_tier() {
        // The CI invariant, stated deny-by-default: a source in the ship namespace
        // may bear ONLY an explicitly ship-safe tier. Written this way, adding a
        // new dangerous `Tier` variant without classifying it here fails the test,
        // rather than silently slipping into the ship namespace.
        let ledger = Ledger::load_embedded().expect("embedded manifest parses");
        for entry in ledger.entries() {
            let ship_safe = matches!(
                entry.tier,
                Tier::System | Tier::PublicDomain | Tier::Cc0 | Tier::Permissive | Tier::ShareAlike
            );
            if entry.namespace == Namespace::Ship {
                assert!(
                    ship_safe,
                    "ship-namespace source '{}' has non-ship-safe tier {:?}",
                    entry.id, entry.tier
                );
            }
        }
    }
}
