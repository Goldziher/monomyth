//! The monomyth domain model and its pure, deterministic engine.
//!
//! `monomyth-core` is the serializable contract every other crate shares. A
//! generator produces a [`World`]; a frontend renders it; the engine mutates it
//! through [`apply`]. The serialized world is the *only* thing that crosses those
//! boundaries, which is what lets a text frontend and a future pixel frontend be
//! drop-in alternatives.
//!
//! This crate is deliberately narrow: **no IO, no async, no rendering, and no
//! LLM/RAG concerns.** It holds only structure and the rules that transform it.
//!
//! # Design invariants
//!
//! - **Deterministic serialization.** Cross-referenced elements live in
//!   [`slotmap::SlotMap`]s keyed by the typed ids in [`LocationId`] et al., never
//!   by `Vec` index; auxiliary relations use [`BTreeMap`](std::collections::BTreeMap)
//!   / [`BTreeSet`](std::collections::BTreeSet) so output is stable and
//!   snapshot-testable.
//! - **Deterministic randomness.** [`RngState`] stores a seed and stream offset
//!   and reconstructs a `ChaCha8` RNG on demand, so a session replays exactly from
//!   `(seed, action-log)`. This procedural stream is separate from the quarantined
//!   content phase.
//! - **The hybrid seam.** Every content-bearing field is a [`Content`] slot that
//!   is [`Empty`](Content::Empty) until the content phase fills it, so structure
//!   and prose regenerate independently.
//!
//! # Grounding
//!
//! The story vocabulary is not invented: [`NarrativeNode`], [`Quest`], and the
//! character facets on [`Entity`] are bound to the framework artifacts through
//! `monomyth-frameworks`.
//!
//! # Example
//!
//! ```
//! use monomyth_core::{apply, Action, Event};
//! # use monomyth_core::doc_support::single_room_world;
//!
//! let mut world = single_room_world();
//! let events = apply(&mut world, Action::Wait)?;
//! assert_eq!(events, vec![Event::Waited]);
//! assert_eq!(world.state.turn, 1);
//! # Ok::<(), monomyth_core::ActionError>(())
//! ```

#![forbid(unsafe_code)]

#[doc(hidden)]
pub mod doc_support;

mod content;
mod engine;
mod entity;
mod ids;
mod narrative;
mod narrative_edit;
mod rng;
mod scored;
mod story;
mod validate;
mod weight;
mod world;

pub use content::{Content, ContentKind, ContentPrompt, Provenance, ProvenanceSource};
pub use engine::{Action, ActionError, Event, ExamineTarget, apply};
pub use entity::{DEFAULT_MAX_HEALTH, Entity, EntityKind, Item, Player};
pub use ids::{EntityId, ItemId, LocationId, NarrativeNodeId, QuestId};
pub use narrative::{
    EdgeKind, NarrativeEdge, NarrativeError, NarrativeNode, NarrativeStructure, NodeKind,
};
pub use narrative_edit::{EditError, EditOutcome, NarrativeEdit, NodeSpec};
pub use rng::RngState;
pub use scored::{ScoredOne, ScoredSet};
pub use story::{Quest, Story};
pub use validate::{LoadError, WorldError};
pub use weight::Weight;
pub use world::{Direction, Location, SCHEMA_VERSION, World, WorldMeta, WorldState};
