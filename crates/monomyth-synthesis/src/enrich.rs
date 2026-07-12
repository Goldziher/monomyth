//! Framework-vocabulary query enrichment.
//!
//! Multi-query retrieval ([`crate::gather_grounding`]) covers the whole arc only
//! if it is *asked* about the whole arc. Left to a single seed query, retrieval
//! clusters on one facet; the judge loop then claws coverage back one
//! `missing_phases` round at a time. Enrichment front-loads that coverage: it
//! turns a taxonomy monomyth already owns (e.g. Campbell's macro-arc stages)
//! into one coverage sub-query per stage, so the initial grounding spans the arc
//! by construction rather than by iteration.
//!
//! This is the monomyth-native analogue of a knowledge-graph query expansion:
//! the "graph" is our own hand-encoded framework artifact, read through
//! [`monomyth_frameworks`]. Enrichment is deliberately **opt-in and
//! domain-keyed** ([`CoverageFramework`]) so the synthesis pipeline stays
//! domain-general — a non-myth `--domain` simply supplies no coverage framework.

use monomyth_frameworks::MonomythStage;

/// A named framework taxonomy whose item vocabulary seeds coverage sub-queries.
///
/// Kept as a small closed enum rather than a free-form string so an unknown
/// framework is rejected at the boundary ([`from_key`](Self::from_key)) instead
/// of silently producing no queries. New tiers (Booker plots, ATU categories)
/// slot in here as they become useful for a given `--domain`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoverageFramework {
    /// Campbell's seventeen monomyth stages (the macro-arc spine).
    Campbell,
}

impl CoverageFramework {
    /// Parse a CLI/config key into a [`CoverageFramework`], or `None` if the key
    /// names no known framework.
    ///
    /// Matching is case-insensitive so `"Campbell"` and `"campbell"` both
    /// resolve.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        match key.trim().to_lowercase().as_str() {
            "campbell" => Some(Self::Campbell),
            _ => None,
        }
    }

    /// The keys [`from_key`](Self::from_key) accepts, for building a helpful
    /// error message at the call site.
    #[must_use]
    pub const fn known_keys() -> &'static [&'static str] {
        &["campbell"]
    }
}

/// Build one coverage sub-query per item of `framework`, in the framework's own
/// (narrative) order.
///
/// For [`CoverageFramework::Campbell`] this yields the seventeen stage names
/// (`"Call to Adventure"`, `"Refusal of the Call"`, …). Bare stage names are
/// used as retrieval probes rather than sentences: they match the loop's own
/// targeted-retrieval style (the judge's `missing_phases` are also short names),
/// and [`crate::gather_grounding`] already dedups and caps the union, so more
/// probes cost coverage, not prompt bloat.
#[must_use]
pub fn coverage_sub_queries(framework: CoverageFramework) -> Vec<String> {
    match framework {
        CoverageFramework::Campbell => MonomythStage::all()
            .iter()
            .map(|stage| stage.info().name.clone())
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn campbell_yields_one_query_per_monomyth_stage_in_order() {
        let queries = coverage_sub_queries(CoverageFramework::Campbell);
        assert_eq!(
            queries.len(),
            MonomythStage::all().len(),
            "one coverage query per Campbell stage"
        );
        assert_eq!(queries.len(), 17, "Campbell has seventeen stages");
        assert_eq!(
            queries[0],
            MonomythStage::CallToAdventure.info().name,
            "queries follow the framework's own narrative order"
        );
    }

    #[test]
    fn from_key_resolves_known_frameworks_case_insensitively() {
        assert_eq!(
            CoverageFramework::from_key("campbell"),
            Some(CoverageFramework::Campbell)
        );
        assert_eq!(
            CoverageFramework::from_key("  Campbell  "),
            Some(CoverageFramework::Campbell)
        );
    }

    #[test]
    fn from_key_rejects_an_unknown_framework() {
        assert_eq!(CoverageFramework::from_key("bogus"), None);
        assert!(!CoverageFramework::known_keys().is_empty());
    }
}
