//! The hybrid-generation seam: content slots that separate *structure* from *prose*.
//!
//! monomyth generates in two physically separated phases. A deterministic
//! procedural pass owns structure (maps, stats, quest skeletons); a
//! non-deterministic content pass (an LLM) fills prose. To let either phase
//! regenerate independently, every content-bearing field is a [`Content`] slot:
//! it is created [`Empty`](Content::Empty) with a [`ContentPrompt`] describing
//! what belongs there, and later transitions to
//! [`Filled`](Content::Filled) carrying the value plus its [`Provenance`]. The
//! content pass can only turn empty into filled; it can never invent structure.

use serde::{Deserialize, Serialize};

/// What kind of prose a [`ContentPrompt`] asks for.
///
/// The kind lets a content generator route a slot to the right prompt template
/// and validate the shape of what it produces (a name is short, a description is
/// a paragraph) without inspecting the surrounding structure.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ContentKind {
    /// A work or world title.
    Title,
    /// A proper name for a location, entity, or item.
    Name,
    /// A descriptive paragraph (room, entity, or item body text).
    Description,
    /// A one- or two-sentence beat synopsis.
    Synopsis,
    /// A line of spoken dialogue.
    Dialogue,
}

/// A description of the prose a [`Content`] slot needs, independent of any value.
///
/// The prompt is authored by the procedural pass and is stable: it survives even
/// after the slot is filled, so a value can be discarded and regenerated from the
/// same instructions. `hint` is free-form guidance grounding the generation in
/// the surrounding structure (e.g. `"a windswept threshold guarded by a sphinx"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentPrompt {
    /// The kind of prose requested.
    pub kind: ContentKind,
    /// Free-form grounding guidance for the generator.
    pub hint: String,
}

impl ContentPrompt {
    /// Build a prompt of `kind` with a grounding `hint`.
    ///
    /// ```
    /// use monomyth_core::{ContentKind, ContentPrompt};
    ///
    /// let prompt = ContentPrompt::new(ContentKind::Name, "a river-god's title");
    /// assert_eq!(prompt.kind, ContentKind::Name);
    /// ```
    #[must_use]
    pub fn new(kind: ContentKind, hint: impl Into<String>) -> Self {
        Self {
            kind,
            hint: hint.into(),
        }
    }
}

/// Where a filled [`Content`] value came from, so provenance travels with prose.
///
/// The two phases must never blur: a procedurally chosen value and an
/// LLM-authored value are auditable and independently regenerable. Recording the
/// source (and, for an LLM, the model) keeps that seam explicit.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProvenanceSource {
    /// Produced by the deterministic procedural pass.
    Procedural,
    /// Produced by the content pass using the named model.
    Llm {
        /// Identifier of the model that authored the value.
        model: String,
    },
}

/// How a filled value was produced, recorded alongside the value itself.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Provenance {
    /// The generation phase that produced the value.
    pub source: ProvenanceSource,
    /// Free-form notes (seed sub-stream, prompt id, review status, …).
    pub notes: String,
}

impl Provenance {
    /// Provenance for a value chosen by the deterministic procedural pass.
    ///
    /// ```
    /// use monomyth_core::{Provenance, ProvenanceSource};
    ///
    /// let provenance = Provenance::procedural("name-table draw");
    /// assert_eq!(provenance.source, ProvenanceSource::Procedural);
    /// ```
    #[must_use]
    pub fn procedural(notes: impl Into<String>) -> Self {
        Self {
            source: ProvenanceSource::Procedural,
            notes: notes.into(),
        }
    }

    /// Provenance for a value authored by the content pass using `model`.
    #[must_use]
    pub fn llm(model: impl Into<String>, notes: impl Into<String>) -> Self {
        Self {
            source: ProvenanceSource::Llm {
                model: model.into(),
            },
            notes: notes.into(),
        }
    }
}

/// A content slot: an [`Empty`](Content::Empty) structural placeholder or a
/// [`Filled`](Content::Filled) value with its provenance.
///
/// Both variants retain the [`ContentPrompt`], so filling is reversible: the
/// content pass can be re-run over an already-filled world by resetting slots to
/// empty and generating again. The value type defaults to [`String`] (prose) but
/// is generic so structured content can reuse the same seam.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Content<T = String> {
    /// An unfilled slot awaiting content generation.
    Empty {
        /// Instructions describing what belongs here.
        prompt: ContentPrompt,
    },
    /// A filled slot carrying its value and how the value was produced.
    Filled {
        /// The generated value.
        value: T,
        /// The instructions the value was generated from (retained for re-runs).
        prompt: ContentPrompt,
        /// How the value was produced.
        provenance: Provenance,
    },
}

impl<T> Content<T> {
    /// Build an empty slot from its prompt.
    #[must_use]
    pub const fn empty(prompt: ContentPrompt) -> Self {
        Self::Empty { prompt }
    }

    /// Build an already-filled slot.
    #[must_use]
    pub const fn filled(value: T, prompt: ContentPrompt, provenance: Provenance) -> Self {
        Self::Filled {
            value,
            prompt,
            provenance,
        }
    }

    /// Whether this slot has a value.
    #[must_use]
    pub const fn is_filled(&self) -> bool {
        matches!(self, Self::Filled { .. })
    }

    /// The value, or `None` while the slot is still empty.
    ///
    /// ```
    /// use monomyth_core::{Content, ContentKind, ContentPrompt, Provenance};
    ///
    /// fn demo() -> Option<()> {
    ///     let mut slot: Content = Content::empty(ContentPrompt::new(ContentKind::Title, "a saga"));
    ///     assert!(slot.value().is_none());
    ///     slot.fill("The Golden Bough".to_string(), Provenance::procedural("seeded"));
    ///     assert_eq!(slot.value()?, "The Golden Bough");
    ///     Some(())
    /// }
    /// assert_eq!(demo(), Some(()));
    /// ```
    #[must_use]
    pub const fn value(&self) -> Option<&T> {
        match self {
            Self::Empty { .. } => None,
            Self::Filled { value, .. } => Some(value),
        }
    }

    /// The provenance of the value, or `None` while the slot is still empty.
    #[must_use]
    pub const fn provenance(&self) -> Option<&Provenance> {
        match self {
            Self::Empty { .. } => None,
            Self::Filled { provenance, .. } => Some(provenance),
        }
    }

    /// The prompt, available in both states (retained after filling).
    #[must_use]
    pub const fn prompt(&self) -> &ContentPrompt {
        match self {
            Self::Empty { prompt } | Self::Filled { prompt, .. } => prompt,
        }
    }

    /// Fill the slot with `value` and its `provenance`, keeping the prompt.
    ///
    /// Overwrites any existing value; the retained prompt makes the fill
    /// idempotent for re-runs of the content pass.
    pub fn fill(&mut self, value: T, provenance: Provenance) {
        // Take the prompt out of the current state without cloning `T`.
        let prompt = match std::mem::replace(self, Self::placeholder()) {
            Self::Empty { prompt } | Self::Filled { prompt, .. } => prompt,
        };
        *self = Self::Filled {
            value,
            prompt,
            provenance,
        };
    }

    /// A throwaway empty slot used only to move a prompt out during [`fill`](Self::fill).
    fn placeholder() -> Self {
        Self::Empty {
            prompt: ContentPrompt {
                kind: ContentKind::Description,
                hint: String::new(),
            },
        }
    }
}
