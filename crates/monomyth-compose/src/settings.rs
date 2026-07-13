//! Pipeline-wide knobs for the compose pipeline.
//!
//! Consolidates the parameters that used to be threaded individually
//! (`max_turns`, then `GROUNDING_TOP_K`, now the revise-loop threshold and
//! cap) into one struct. Three independent signature changes across this
//! crate's short history is the point at which a settings struct earns its
//! keep over another loose parameter.

/// The pipeline knobs for [`crate::compose`]/[`crate::draft::draft`].
///
/// These are interim, hand-set defaults: destined to move into the
/// `[compose]` config section (ADR-0015 / ADR-0027) once compose grows a
/// config-driven entry point. Kept as named struct fields here rather than
/// call-site literals in the meantime.
#[derive(Clone, Debug, PartialEq)]
pub struct ComposeSettings {
    /// The maximum number of model turns [`crate::generate::generate_long_form`]
    /// may take to narrate a single section.
    pub max_turns: usize,
    /// The number of REFERENCE-path passages retrieved as grounding for each
    /// outline section.
    pub grounding_top_k: u32,
    /// The minimum cosine similarity (against the mean of a section's
    /// grounding-passage embeddings) a drafted section must reach to be
    /// accepted without revision.
    pub revise_threshold: f64,
    /// The maximum number of revision attempts made after the initial draft
    /// when a section's score stays below [`Self::revise_threshold`]. The
    /// total number of drafting attempts for a section is therefore at most
    /// `1 + max_revise_iterations`.
    pub max_revise_iterations: usize,
}

impl Default for ComposeSettings {
    /// Production defaults for the compose pipeline.
    fn default() -> Self {
        Self {
            max_turns: crate::generate::DEFAULT_MAX_TURNS,
            // Formerly `draft::GROUNDING_TOP_K`; moved here now that the
            // pipeline's knobs are consolidated into one settings struct.
            grounding_top_k: 4,
            revise_threshold: 0.6,
            max_revise_iterations: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ComposeSettings;

    #[test]
    fn should_default_to_the_documented_production_values() {
        let settings = ComposeSettings::default();

        assert_eq!(settings.max_turns, 5);
        assert_eq!(settings.grounding_top_k, 4);
        assert!(
            (settings.revise_threshold - 0.6).abs() < f64::EPSILON,
            "expected revise_threshold ~0.6, got {}",
            settings.revise_threshold
        );
        assert_eq!(settings.max_revise_iterations, 2);
    }
}
