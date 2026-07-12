//! The LLM's structured-output DTO for a synthesized law candidate.
//!
//! [`CandidateLaw`] is deliberately narrow: it carries only the abstract
//! structural taxonomy the model is asked to distill (a title, and per-item
//! names/descriptions). It carries no licensing metadata, no item ids, and no
//! reviewer field, because the model's authority under ADR-0016 is limited to
//! the *idea* — the stage/role/phase vocabulary and its one-sentence structural
//! description. Ids, provenance, and licensing are stamped onto the eventual
//! [`monomyth_frameworks::LawArtifact`] by [`crate::pipeline::draft_law`], which
//! the model never sees and cannot influence.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A candidate structural law, as distilled by the LLM from reference priors.
///
/// One level of nesting (`items: Vec<CandidateItem>`) is supported by the
/// Gemini schema sanitizer's `$defs`/`$ref` inlining (see
/// `monomyth_llm::backend::sanitize_schema_for_model`), so this DTO can safely
/// carry a list of structured items rather than flattening everything into a
/// single string field.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CandidateLaw {
    /// A human-readable title for the taxonomy (e.g. "Three-Act Structure").
    ///
    /// Not a machine id: the pipeline assigns the stable `law` identifier from
    /// [`crate::pipeline::DraftRequest::law_id`], which the model never sees.
    pub title: String,
    /// The taxonomy's ordered items (e.g. stages, roles, or phases).
    ///
    /// Unordered from the model's perspective — the pipeline stamps sequential
    /// ids onto this list in the order returned, so the model's only authority
    /// is the content and relative ordering of the items themselves.
    pub items: Vec<CandidateItem>,
}

/// One item of a [`CandidateLaw`]: a named structural element with a short,
/// abstract description.
///
/// No id field: stable, contiguous ids are a pipeline concern
/// ([`monomyth_frameworks::LawItem::id`]), not something the model should
/// invent or be trusted to keep contiguous.
#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
pub struct CandidateItem {
    /// The item's canonical name (e.g. "Setup", "Confrontation").
    pub name: String,
    /// A one-sentence, abstract structural description — general vocabulary
    /// only, never a quotation or close paraphrase of a reference passage.
    pub description: String,
    /// Whether this item's structure was actually derivable from the grounding.
    ///
    /// The model sets this to `false` — an honest "could not derive from
    /// grounding" abstention — when it can name a structurally-expected phase
    /// that the grounding does not support, in preference to inventing content.
    /// The judge rewards this over a fabricated phase. Defaults to `true`, so an
    /// output that omits the field (or an older fixture) parses as derivable.
    #[serde(default = "derivable_default")]
    pub derivable: bool,
}

/// The default for [`CandidateItem::derivable`]: an item is derivable unless the
/// model explicitly abstains. A free function because `#[serde(default = …)]`
/// needs a path, and `bool::default()` is `false` (the wrong polarity here).
fn derivable_default() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::CandidateItem;

    #[test]
    fn an_item_omitting_derivable_deserializes_as_derivable() {
        let item: CandidateItem =
            serde_json::from_str(r#"{"name": "Departure", "description": "The hero leaves."}"#)
                .expect("valid candidate item JSON");
        assert!(
            item.derivable,
            "an omitted derivable field must default to true, not bool::default() false"
        );
    }

    #[test]
    fn an_item_may_explicitly_abstain() {
        let item: CandidateItem = serde_json::from_str(
            r#"{"name": "Apotheosis", "description": "A godhead phase.", "derivable": false}"#,
        )
        .expect("valid candidate item JSON");
        assert!(
            !item.derivable,
            "an explicit derivable:false must be honored"
        );
    }
}
