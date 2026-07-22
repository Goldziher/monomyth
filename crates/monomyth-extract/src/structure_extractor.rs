//! [`MinimalStructureExtractor`] — the [`StructureExtractor`] slice described in
//! `monomyth-contracts` (ADR-0018, Phase 3 of the roadmap): derive a **minimal
//! valid** [`World`] (one narrative node, one location) directly from raw
//! source text, via a single structured LLM call grounded in the Campbell
//! macro-tier vocabulary (`monomyth-frameworks`) and optionally biased by a
//! `monomyth-genre` [`GenreProfile`].
//!
//! This is deliberately the smallest slice that proves the frozen contract can
//! hold *extracted* structure, not just a re-classified label (which
//! [`StageReclassifyingExtractor`](crate::StageReclassifyingExtractor) already
//! proves): exactly one node, exactly one location, no edges, no entities, no
//! items, no quests. Scaling extraction to multi-beat structure is future work.
//!
//! # Write surface
//!
//! The extracted node's *content* (label, stage, synopsis hint; situation,
//! functions, and motifs are left unrealized in this minimal slice, matching
//! `NodeSpec::new`) is written through
//! [`NarrativeEdit::AddNode`](monomyth_core::NarrativeEdit::AddNode) and its
//! [`NodeSpec`] — the same write surface generation uses. Bootstrapping the
//! very first node as the structure's root and sole ending has no
//! `NarrativeEdit` of its own: no codepath in this workspace expresses that
//! step as an edit, including generation's own `BackbonePass`
//! (`monomyth-gen/src/passes/backbone.rs`), which builds a fresh
//! `NarrativeStructure` directly and assigns `.root`/`.endings` before
//! installing it on the `World` — there is nothing pre-existing yet for an
//! edit to apply to. [`bootstrap_structure`] follows that same precedent for
//! this one step; everything about the node's *extracted content* goes
//! through the edit. Likewise, [`Location`] has no
//! edit vocabulary at all (mirroring `monomyth-gen`'s `MapPass`), so it is
//! constructed directly. The assembled result is gated by [`World::validate`]
//! before it is ever returned — exactly like generation's own output.

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use monomyth_contracts::{StructureExtractError, StructureExtractor};
use monomyth_core::{
    Content, ContentKind, ContentPrompt, EditOutcome, Location, LocationId, NarrativeEdit,
    NarrativeStructure, NodeSpec, Player, Provenance, RngState, SCHEMA_VERSION, Story, World,
    WorldMeta, WorldState,
};
use monomyth_frameworks::MonomythStage;
use monomyth_genre::{GenreKind, GenreProfile};
use monomyth_llm::Llm;
use schemars::JsonSchema;
use serde::Deserialize;
use slotmap::SlotMap;

/// The schema name recorded on every generation call this extractor makes.
const SCHEMA_NAME: &str = "minimal_structure_beat";

/// The single structured beat the model must return: everything this minimal
/// slice needs to populate one [`NodeSpec`] and one [`Location`].
#[derive(Debug, Deserialize, JsonSchema)]
struct ExtractedBeat {
    /// A short, stable structural label for the beat (mirrors [`NodeSpec::label`]).
    node_label: String,
    /// The artifact `id` of the Campbell stage ([`MonomythStage::id`]) this beat
    /// realizes.
    stage_id: u16,
    /// A one- or two-sentence synopsis of the beat, used as the node's
    /// [`ContentKind::Synopsis`] hint.
    synopsis_hint: String,
    /// The proper name of the single location where the beat takes place.
    location_name: String,
    /// A one-paragraph description of that location.
    location_description: String,
}

/// The writing frame handed to the model ahead of the source text, selected by
/// [`GenreKind`] so the same extractor reads a passage through a
/// genre-specific lens without a medium-specific branch anywhere else.
fn genre_framing(kind: GenreKind) -> &'static str {
    match kind {
        GenreKind::Myth => {
            "You are extracting the narrative skeleton of a comparative-mythology tale."
        }
        GenreKind::Detective => "You are extracting the narrative skeleton of a detective mystery.",
        GenreKind::LitRpg => "You are extracting the narrative skeleton of a LitRPG adventure.",
    }
}

/// Render the Campbell macro-tier vocabulary as `"id = Name"` lines, so the
/// model is constrained to choose a stage id that actually exists in
/// `monomyth-frameworks` rather than inventing one.
fn stage_catalog() -> String {
    MonomythStage::all()
        .iter()
        .map(|stage| format!("{} = {}", stage.id(), stage.info().name))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Build the deterministic extraction prompt for `text` under `framing`.
///
/// A pure function of its inputs (plus the static framework vocabulary), so
/// two calls with the same arguments always produce byte-identical prompts —
/// the property a cassette-replayed test depends on.
fn build_prompt(text: &str, framing: &str) -> String {
    format!(
        "{framing}\n\n\
         Identify the single most central narrative beat in the passage below, \
         and the one location where it takes place.\n\n\
         Passage:\n{text}\n\n\
         Choose exactly one Campbell stage id from this list (the vocabulary is \
         fixed; do not invent an id outside it):\n{}\n\n\
         Respond with the beat's structural label, the chosen stage id, a short \
         synopsis hint grounding its prose, and the location's proper name and \
         a one-paragraph description.",
        stage_catalog(),
    )
}

/// Bootstrap a single-node [`NarrativeStructure`] whose sole node is both root
/// and sole ending, from `spec`.
///
/// See the module doc for why this one step bypasses `NarrativeEdit`.
fn bootstrap_structure(spec: NodeSpec) -> Result<NarrativeStructure, StructureExtractError> {
    let mut structure = NarrativeStructure::default();
    let outcome = structure
        .apply_edit(&NarrativeEdit::AddNode { spec })
        .map_err(|error| StructureExtractError::Bootstrap(error.to_string()))?;
    let EditOutcome::NodeAdded(root) = outcome else {
        return Err(StructureExtractError::Bootstrap(
            "AddNode did not report a new node id".to_owned(),
        ));
    };
    structure.root = root;
    structure.endings.insert(root);
    structure.recompute_kinds();
    Ok(structure)
}

/// Derive a minimal valid [`World`] from raw source text via one structured
/// LLM call.
///
/// Borrows its [`Llm`] client and model label for the duration of a call,
/// mirroring `monomyth-gen`'s `ContentContext` (the [`Llm`]'s own configured
/// model is not exposed through its API, so the caller supplies the same
/// label it configured the client with). The optional [`GenreProfile`] only
/// ever selects the prompt's [`genre_framing`]; it never crosses into
/// `monomyth-core` or `monomyth-contracts`.
#[derive(Debug)]
pub struct MinimalStructureExtractor<'a> {
    llm: &'a Llm,
    model: &'a str,
    genre: Option<&'a GenreProfile>,
}

impl<'a> MinimalStructureExtractor<'a> {
    /// Build an extractor over `llm`, targeting the default (myth) genre framing.
    #[must_use]
    pub fn new(llm: &'a Llm, model: &'a str) -> Self {
        Self {
            llm,
            model,
            genre: None,
        }
    }

    /// Build an extractor over `llm`, framing the extraction prompt with `genre`.
    #[must_use]
    pub fn with_genre(llm: &'a Llm, model: &'a str, genre: &'a GenreProfile) -> Self {
        Self {
            llm,
            model,
            genre: Some(genre),
        }
    }

    /// The genre framing this extractor prompts with (myth, absent a configured
    /// [`GenreProfile`]).
    fn framing(&self) -> &'static str {
        genre_framing(self.genre.map_or(GenreKind::Myth, |profile| profile.kind))
    }
}

#[async_trait]
impl StructureExtractor for MinimalStructureExtractor<'_> {
    async fn extract_structure(&self, text: &str) -> Result<World, StructureExtractError> {
        let prompt = build_prompt(text, self.framing());
        let generated = self
            .llm
            .generate::<ExtractedBeat>(&prompt, SCHEMA_NAME)
            .await
            .map_err(|error| StructureExtractError::Generation(error.to_string()))?;
        let beat = generated.value;

        let stage = MonomythStage::from_id(beat.stage_id).ok_or_else(|| {
            StructureExtractError::UnrecognizedLabel(format!("Campbell stage id {}", beat.stage_id))
        })?;

        let structure =
            bootstrap_structure(NodeSpec::new(beat.node_label, stage, beat.synopsis_hint))?;
        let root = structure.root();

        let mut locations: SlotMap<LocationId, Location> = SlotMap::default();
        let location_id = locations.insert(Location {
            name: Content::filled(
                beat.location_name,
                ContentPrompt::new(ContentKind::Name, "the extracted location's proper name"),
                Provenance::llm(self.model, "MinimalStructureExtractor"),
            ),
            description: Content::filled(
                beat.location_description,
                ContentPrompt::new(
                    ContentKind::Description,
                    "the extracted location's description",
                ),
                Provenance::llm(self.model, "MinimalStructureExtractor"),
            ),
            exits: BTreeMap::new(),
            entities: BTreeSet::new(),
            items: BTreeSet::new(),
        });

        let world = World {
            meta: WorldMeta {
                seed: 0,
                schema_version: SCHEMA_VERSION,
                title: Content::empty(ContentPrompt::new(
                    ContentKind::Title,
                    "the extracted world's title",
                )),
            },
            locations,
            entities: SlotMap::default(),
            items: SlotMap::default(),
            player: Player::new(location_id),
            story: Story {
                structure,
                plot: None,
                quests: SlotMap::default(),
            },
            state: WorldState {
                cursor: root,
                ..WorldState::default()
            },
            rng: RngState::new(0),
        };

        world.validate()?;
        Ok(world)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn genre_framing_should_differ_by_genre_kind() {
        let myth = genre_framing(GenreKind::Myth);
        let detective = genre_framing(GenreKind::Detective);
        let litrpg = genre_framing(GenreKind::LitRpg);
        assert_ne!(myth, detective);
        assert_ne!(myth, litrpg);
        assert_ne!(detective, litrpg);
    }

    #[test]
    fn stage_catalog_should_list_every_campbell_stage_by_id() {
        let catalog = stage_catalog();
        for stage in MonomythStage::all() {
            let line = format!("{} = {}", stage.id(), stage.info().name);
            assert!(
                catalog.contains(&line),
                "catalog must contain {line:?}, got: {catalog}"
            );
        }
    }

    #[test]
    fn build_prompt_should_be_a_pure_function_of_its_inputs() {
        let first = build_prompt("a passage", "framing");
        let second = build_prompt("a passage", "framing");
        assert_eq!(
            first, second,
            "identical inputs must yield identical prompts"
        );

        let different_text = build_prompt("a different passage", "framing");
        assert_ne!(first, different_text);

        let different_framing = build_prompt("a passage", "different framing");
        assert_ne!(first, different_framing);
    }

    #[test]
    fn bootstrap_structure_should_make_the_new_node_root_and_sole_ending() {
        let spec = NodeSpec::new(
            "CallToAdventure",
            MonomythStage::CallToAdventure,
            "the call",
        );
        let structure = bootstrap_structure(spec).expect("bootstrapping a fresh spec succeeds");

        let root = structure.root();
        assert_eq!(structure.nodes.len(), 1);
        assert!(structure.nodes.contains_key(root));
        assert_eq!(structure.endings, BTreeSet::from([root]));
        structure
            .validate()
            .expect("a single-node structure is valid");
    }
}
