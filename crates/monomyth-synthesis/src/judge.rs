//! The LLM-as-judge that scores a drafted candidate and drives the feedback
//! loop in [`crate::pipeline::draft_law`].
//!
//! A single distillation pass tends to under-collect: a source that supports a
//! dozen distinct macro-phases gets flattened into a handful of coarse
//! buckets. Rather than hand-tune the distillation prompt until it happens to
//! work, the pipeline asks a second model call to grade the candidate against
//! named criteria and to name the specific structural phases it judges
//! missing. Those two signals — a weighted score and a list of missing phases
//! — are what let [`crate::pipeline::draft_law`] decide whether to accept a
//! candidate and, if not, what to go retrieve grounding for next.

use monomyth_knowledge::Passage;
use monomyth_llm::{Generated, Llm};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::candidate::CandidateLaw;
use crate::error::SynthesisError;

/// The `schemars`/`Llm::generate` schema name for [`JudgeVerdict`].
const JUDGE_SCHEMA_NAME: &str = "JudgeVerdict";

/// One scoring criterion the judge applies to a candidate law.
///
/// `weight` is a relative importance multiplier used by [`weighted_score`]; it
/// is not itself a probability or a bound on `score`, only a coefficient in
/// the weighted average.
#[derive(Clone, Debug)]
pub struct LawCriterion {
    /// The criterion's stable name (matched case-insensitively against the
    /// judge's [`CriterionScore::name`]).
    pub name: &'static str,
    /// Instructions shown to the judge model describing what the criterion
    /// means and how to score it.
    pub instructions: &'static str,
    /// The criterion's weight in [`weighted_score`]'s weighted average.
    pub weight: f64,
}

/// The default criteria set for judging a synthesized structural law.
///
/// These are tuned for LAW extraction specifically (an abstract macro-tier
/// taxonomy), not prose quality: the dominant failure mode observed in
/// practice is under-collection — a source that supports on the order of a
/// dozen distinct phases gets flattened into three or four coarse buckets —
/// so [`Exhaustiveness`](Self) and [`Tier fit`](Self) carry the highest
/// weights.
pub const DEFAULT_CRITERIA: &[LawCriterion] = &[
    LawCriterion {
        name: "Exhaustiveness",
        instructions: "Every distinct structural phase present in the grounding is captured as \
                        its own item, in narrative order, from departure through the trials/crux \
                        to the return and restoration. Distinct phases must not be merged into \
                        coarse buckets: if the grounding supports separating a beat into two \
                        phases, they must appear as two items.",
        weight: 1.0,
    },
    LawCriterion {
        name: "Source grounding",
        instructions: "Every phase is supported by the grounding passages; nothing is invented \
                        that the passages do not imply.",
        weight: 0.9,
    },
    LawCriterion {
        name: "Ordering & non-overlap",
        instructions: "Phases appear in narrative order and are mutually distinct: no two items \
                        describe the same beat under different names.",
        weight: 0.7,
    },
    LawCriterion {
        name: "Abstraction",
        instructions: "Phases are abstract structural descriptions using general stage/role/phase \
                        vocabulary: no proper nouns, and no near-verbatim source wording.",
        weight: 0.8,
    },
    LawCriterion {
        name: "Tier fit",
        instructions: "Granularity matches the macro tier: a full hero-journey arc has on the \
                        order of a dozen distinct macro-phases, not a handful.",
        weight: 0.6,
    },
    LawCriterion {
        name: "Honest abstention",
        instructions: "An item explicitly marked [NOT DERIVABLE FROM GROUNDING] is an HONEST \
                        abstention for a structurally-expected phase the grounding does not \
                        support, and is CORRECT behaviour: score it high. Reward naming a gap \
                        over inventing an unsupported phase. Penalize a fabricated phase the \
                        grounding does not imply; never penalize an honest abstention marker.",
        weight: 0.6,
    },
];

/// The judge's score for one [`LawCriterion`], on a 0-100 scale.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct CriterionScore {
    /// The criterion name this score applies to (matched against
    /// [`LawCriterion::name`] by [`weighted_score`]).
    pub name: String,
    /// The score, 0-100.
    pub score: u8,
    /// The judge's rationale for the score, for audit and prompt-debugging.
    pub rationale: String,
}

/// The judge's full assessment of one candidate law.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct JudgeVerdict {
    /// Per-criterion scores; see [`weighted_score`] for how these combine.
    pub criteria: Vec<CriterionScore>,
    /// Named structural phases the judge finds absent or underdeveloped in
    /// the candidate. Drives the next targeted retrieval in the feedback
    /// loop.
    pub missing_phases: Vec<String>,
    /// Concrete instructions for improving the candidate on its next draft.
    pub instructions: Vec<String>,
}

/// Ask `llm` to judge `candidate` against `criteria`, given the `grounding`
/// passages it was distilled from.
///
/// The prompt frames the model as an evaluator (not an author): it is shown
/// the criteria with their instructions, the candidate rendered as
/// human-readable text, and the grounding passages, then asked to score each
/// criterion, name missing phases, and give improvement instructions.
///
/// # Errors
///
/// Returns [`SynthesisError::Generation`] if the underlying LLM call fails or
/// its output does not parse as a [`JudgeVerdict`].
pub async fn judge_candidate(
    llm: &Llm,
    candidate: &CandidateLaw,
    grounding: &[Passage],
    criteria: &[LawCriterion],
) -> Result<JudgeVerdict, SynthesisError> {
    let prompt = build_judge_prompt(candidate, grounding, criteria);
    let Generated { value, .. } = llm
        .generate::<JudgeVerdict>(&prompt, JUDGE_SCHEMA_NAME)
        .await
        .map_err(SynthesisError::Generation)?;
    Ok(value)
}

/// Build the judge prompt: system-evaluator framing, the criteria with their
/// instructions, the candidate rendered as text, and the grounding passages.
fn build_judge_prompt(
    candidate: &CandidateLaw,
    grounding: &[Passage],
    criteria: &[LawCriterion],
) -> String {
    use std::fmt::Write as _;

    let mut prompt = String::from(
        "You are an EVALUATOR, not an author. Score the candidate structural taxonomy below \
         against each criterion, on a scale of 0 to 100. Be strict: this is a macro-tier law \
         extraction task, and the most common failure is under-collection (merging distinct \
         phases into coarse buckets) rather than over-collection.\n\n\
         For each criterion, report its name, a 0-100 score, and a one-sentence rationale. Also \
         report any named structural phases you judge absent or underdeveloped in the candidate \
         relative to the grounding (`missing_phases`), and concrete instructions for improving \
         the candidate on its next draft (`instructions`).\n\n\
         An item tagged [NOT DERIVABLE FROM GROUNDING] is an honest abstention — the author \
         naming a structurally-expected phase the grounding does not support, rather than \
         inventing one. Treat it as correct, not as a defect.\n\n\
         Criteria:\n",
    );
    for criterion in criteria {
        let _ = writeln!(prompt, "- {}: {}", criterion.name, criterion.instructions);
    }

    prompt.push_str("\nCandidate taxonomy:\n");
    let _ = writeln!(prompt, "Title: {}", candidate.title);
    for (index, item) in candidate.items.iter().enumerate() {
        let position = index + 1;
        let marker = if item.derivable {
            ""
        } else {
            " [NOT DERIVABLE FROM GROUNDING]"
        };
        let _ = writeln!(
            prompt,
            "{position}. {}: {}{marker}",
            item.name, item.description
        );
    }

    prompt.push_str("\nGrounding passages (priors only, never to be reproduced):\n");
    for (index, passage) in grounding.iter().enumerate() {
        let position = index + 1;
        let _ = writeln!(prompt, "\n[{position}] {}", passage.text);
    }

    prompt
}

/// Compute the weighted average score for `verdict` against `criteria`.
///
/// Each [`CriterionScore::name`] is matched (case-insensitively, trimmed)
/// against a [`LawCriterion::name`] in `criteria`; unmatched names are
/// ignored. The result is `sum(score * weight) / sum(weight)` over matched
/// criteria; if nothing matched, returns `0.0`.
///
/// Pure and deterministic: no I/O, no randomness.
#[must_use]
pub fn weighted_score(verdict: &JudgeVerdict, criteria: &[LawCriterion]) -> f64 {
    let mut weighted_sum = 0.0;
    let mut weight_total = 0.0;

    for criterion_score in &verdict.criteria {
        let matched = criteria.iter().find(|criterion| {
            criterion
                .name
                .eq_ignore_ascii_case(criterion_score.name.trim())
        });
        if let Some(criterion) = matched {
            weighted_sum += f64::from(criterion_score.score) * criterion.weight;
            weight_total += criterion.weight;
        }
    }

    if weight_total == 0.0 {
        return 0.0;
    }
    weighted_sum / weight_total
}

#[cfg(test)]
mod tests {
    use super::{CriterionScore, JudgeVerdict, LawCriterion, weighted_score};

    /// A small, fixed criteria set for exact-arithmetic assertions, distinct
    /// from [`super::DEFAULT_CRITERIA`] so the test is independent of any
    /// future tuning of the production weights.
    const TEST_CRITERIA: &[LawCriterion] = &[
        LawCriterion {
            name: "Exhaustiveness",
            instructions: "test criterion",
            weight: 1.0,
        },
        LawCriterion {
            name: "Abstraction",
            instructions: "test criterion",
            weight: 0.5,
        },
    ];

    fn score(name: &str, score: u8) -> CriterionScore {
        CriterionScore {
            name: name.to_owned(),
            score,
            rationale: "because".to_owned(),
        }
    }

    #[test]
    fn computes_the_exact_weighted_average_over_matched_criteria() {
        let verdict = JudgeVerdict {
            criteria: vec![score("Exhaustiveness", 80), score("Abstraction", 40)],
            missing_phases: Vec::new(),
            instructions: Vec::new(),
        };

        // (80*1.0 + 40*0.5) / (1.0 + 0.5) = 100.0 / 1.5 = 66.666...
        let result = weighted_score(&verdict, TEST_CRITERIA);
        assert!(
            (result - 66.666_666_666_666_67).abs() < 1e-9,
            "expected ~66.6667, got {result}"
        );
    }

    #[test]
    fn matches_criterion_names_case_insensitively_and_trims_whitespace() {
        let verdict = JudgeVerdict {
            criteria: vec![score("  exhaustiveness  ", 100), score("ABSTRACTION", 0)],
            missing_phases: Vec::new(),
            instructions: Vec::new(),
        };

        // (100*1.0 + 0*0.5) / 1.5 = 66.666...
        let result = weighted_score(&verdict, TEST_CRITERIA);
        assert!(
            (result - 66.666_666_666_666_67).abs() < 1e-9,
            "got {result}"
        );
    }

    #[test]
    fn ignores_unmatched_criterion_names() {
        let verdict = JudgeVerdict {
            criteria: vec![score("Exhaustiveness", 80), score("Nonexistent", 100)],
            missing_phases: Vec::new(),
            instructions: Vec::new(),
        };

        // Only "Exhaustiveness" matches: 80*1.0 / 1.0 = 80.0.
        assert!((weighted_score(&verdict, TEST_CRITERIA) - 80.0).abs() < 1e-9);
    }

    #[test]
    fn returns_zero_when_nothing_matched() {
        let verdict = JudgeVerdict {
            criteria: vec![score("Nonexistent", 100)],
            missing_phases: Vec::new(),
            instructions: Vec::new(),
        };

        assert!((weighted_score(&verdict, TEST_CRITERIA) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn returns_zero_for_empty_criteria_scores() {
        let verdict = JudgeVerdict {
            criteria: Vec::new(),
            missing_phases: Vec::new(),
            instructions: Vec::new(),
        };

        assert!((weighted_score(&verdict, TEST_CRITERIA) - 0.0).abs() < 1e-9);
    }

    #[test]
    fn judge_prompt_flags_only_the_abstaining_item() {
        use super::{DEFAULT_CRITERIA, build_judge_prompt};
        use crate::candidate::{CandidateItem, CandidateLaw};

        let candidate = CandidateLaw {
            title: "Arc".to_owned(),
            items: vec![
                CandidateItem {
                    name: "Departure".to_owned(),
                    description: "The hero leaves the ordinary world.".to_owned(),
                    derivable: true,
                },
                CandidateItem {
                    name: "Apotheosis".to_owned(),
                    description: "A godhead phase the sources do not support.".to_owned(),
                    derivable: false,
                },
            ],
        };
        let prompt = build_judge_prompt(&candidate, &[], DEFAULT_CRITERIA);

        assert!(
            prompt.contains(
                "Apotheosis: A godhead phase the sources do not support. \
                             [NOT DERIVABLE FROM GROUNDING]"
            ),
            "the abstaining item must be tagged for the judge"
        );
        assert!(
            prompt.contains("Departure: The hero leaves the ordinary world.\n"),
            "the derivable item must NOT carry the abstention tag"
        );
        assert!(
            prompt.contains("Honest abstention"),
            "the abstention criterion must be shown to the judge"
        );
    }
}
