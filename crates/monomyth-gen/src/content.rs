//! The content (LLM) half of generation: filling empty [`Content`] slots.
//!
//! After [`generate_structure`](crate::Generator::generate_structure) lays down a
//! world with every [`Content`](monomyth_core::Content) slot
//! [`Empty`](monomyth_core::Content::Empty), a pipeline of [`ContentPass`]es fills
//! those slots with LLM-authored prose, grounded by ship-safe retrieval. This
//! phase is **non-deterministic** and **quarantined** from the procedural RNG: a
//! content pass takes no `ChaCha8Rng` and never touches
//! [`World::rng`](monomyth_core::World::rng). It fills slots only — it must never
//! add or remove elements, or change exits, roles, keys, or any other structure.
//!
//! Each pass reads a slot's [`ContentPrompt`](monomyth_core::ContentPrompt) hint,
//! retrieves ship-safe grounding passages for it, formats both into an LLM prompt
//! constrained to a small schema DTO ([`NamedProse`] / [`TextProse`]), and fills
//! the slot with [`Provenance::llm`](monomyth_core::Provenance::llm) stamped with
//! the context's model label and a note naming the pass and its grounding sources.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use async_trait::async_trait;
use monomyth_core::World;
use monomyth_knowledge::{Knowledge, KnowledgeQuery, Passage};
use monomyth_llm::Llm;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::error::GenError;

/// Number of ship-safe passages retrieved to ground each content slot.
///
/// A named constant so the grounding breadth is tunable in one place rather than
/// scattered as a literal across the passes.
pub(crate) const GROUNDING_TOP_K: u32 = 4;

/// The dependencies a [`ContentPass`] draws on, borrowed for the duration of a run.
///
/// The `model` label is stamped into every
/// [`Provenance::llm`](monomyth_core::Provenance::llm) the passes record. The
/// [`Llm`]'s own configured model is not exposed through its API, so the caller
/// supplies the same label it configured the client with.
#[derive(Debug)]
pub struct ContentContext<'a> {
    /// The typed structured-generation client the passes prompt.
    pub llm: &'a Llm,
    /// The ship-gated retrieval layer used to ground each slot.
    pub knowledge: &'a Knowledge,
    /// The model label recorded in provenance (matches how `llm` was configured).
    pub model: &'a str,
}

/// One ordered stage of content generation: it fills [`Content`] slots with prose.
///
/// Boxed as a trait object in the [`Generator`](crate::Generator) content
/// pipeline, so the trait requires [`Debug`](std::fmt::Debug) (for the workspace
/// `missing_debug_implementations` lint) and [`Send`] + [`Sync`].
#[async_trait]
pub trait ContentPass: std::fmt::Debug + Send + Sync {
    /// A stable, human-readable name used in provenance notes and diagnostics.
    fn name(&self) -> &'static str;

    /// Fill this pass's slots in `world`, prompting the LLM in `context`.
    ///
    /// The implementation must only turn [`Empty`](monomyth_core::Content::Empty)
    /// slots into [`Filled`](monomyth_core::Content::Filled) ones; it must never
    /// alter structure (elements, exits, roles, keys) or touch the procedural RNG.
    ///
    /// # Errors
    ///
    /// Returns [`GenError::Llm`] if a generation call fails, or
    /// [`GenError::Knowledge`] if a grounding retrieval fails.
    async fn apply(&self, world: &mut World, context: &ContentContext<'_>) -> Result<(), GenError>;
}

/// A named subject: the schema the LLM is constrained to for a location or entity
/// slot pair (its proper name and its descriptive paragraph).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct NamedProse {
    /// The subject's proper name (short).
    pub name: String,
    /// The subject's descriptive paragraph.
    pub description: String,
}

/// A single line of prose: the schema the LLM is constrained to for a standalone
/// text slot such as the world title.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TextProse {
    /// The generated text.
    pub text: String,
}

/// Retrieve ship-safe grounding passages for `query_hint`.
///
/// Grounding is **best-effort**: an empty result (or an empty knowledge base) is
/// fine — the caller still generates from the slot hint alone. The ship filter is
/// applied inside [`Knowledge`], so this never re-implements the licensing gate.
///
/// # Errors
///
/// Returns [`GenError::Knowledge`] if the retrieval itself fails.
pub(crate) async fn ground(
    context: &ContentContext<'_>,
    query_hint: &str,
    top_k: u32,
) -> Result<Vec<Passage>, GenError> {
    let passages = context
        .knowledge
        .retrieve(KnowledgeQuery::surfaceable(query_hint, top_k))
        .await?;
    Ok(passages)
}

/// Format the grounding passages and the slot `hint` into an LLM prompt.
///
/// `instruction` states the writing task (e.g. "Write the title of a world"); the
/// slot hint locates it in the surrounding structure; the passages are offered as
/// ship-safe source material. An empty passage set simply omits the grounding
/// block — the model still has the instruction and hint.
pub(crate) fn build_prompt(instruction: &str, hint: &str, passages: &[Passage]) -> String {
    let mut prompt = String::new();
    // The individual writes target a fresh `String`, so they cannot fail.
    let _ = writeln!(
        prompt,
        "You are writing grounded prose for a mythic text adventure."
    );
    let _ = writeln!(prompt, "\nTask: {instruction}");
    let _ = writeln!(prompt, "Context for this slot: {hint}");
    if passages.is_empty() {
        let _ = writeln!(
            prompt,
            "\nNo grounding passages were retrieved; draw on the context above alone."
        );
    } else {
        let _ = writeln!(
            prompt,
            "\nGrounding passages (ship-safe source material you may draw on):"
        );
        for passage in passages {
            let _ = writeln!(prompt, "- [{}] {}", passage.source_id, passage.text);
        }
    }
    let _ = writeln!(
        prompt,
        "\nReturn only JSON conforming to the required schema."
    );
    prompt
}

/// Build a provenance note naming the `pass` and the grounding sources it drew on.
///
/// Source ids are de-duplicated and ordered for a stable, readable note even when
/// several passages share a source.
pub(crate) fn grounding_note(pass: &str, passages: &[Passage]) -> String {
    let sources: BTreeSet<&str> = passages
        .iter()
        .map(|passage| passage.source_id.as_str())
        .collect();
    if sources.is_empty() {
        format!("{pass}; no grounding sources")
    } else {
        let joined = sources.into_iter().collect::<Vec<_>>().join(", ");
        format!("{pass}; grounded by {joined}")
    }
}
