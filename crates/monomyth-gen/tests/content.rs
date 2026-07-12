//! The content (LLM) half of generation: filling slots, and the hard invariant
//! that it changes *only* content, never structure.
//!
//! These tests use real objects with test doubles (per the no-mocking-internal
//! rule): a real [`Llm`] over a scripted [`StructuredBackend`], and a real
//! [`Knowledge`] over an in-memory store and a deterministic fake embedder.

mod support;

use std::collections::{BTreeMap, BTreeSet};

use async_trait::async_trait;
use monomyth_core::{Direction, EntityId, ItemId, LocationId, ScoredOne, World};
use monomyth_frameworks::{Archetype, MonomythStage, ProppRole};
use monomyth_gen::{ContentConfig, ContentContext, Generator};
use monomyth_knowledge::IngestInput;
use monomyth_llm::{BackendError, Llm, StructuredBackend, Usage};
use serde::Serialize;
use serde_json::{Value, json};
use support::{assert_all_slots_filled_with_llm_provenance, test_knowledge};

/// The seed every test generates from; the 4a determinism tests pin the same one.
const SEED: u64 = 42;
/// The model label the content context stamps into provenance.
const MODEL_LABEL: &str = "test/fake-model";
/// The schema name the title pass requests (see `TitleContentPass`).
const TITLE_SCHEMA: &str = "world_title";
/// The canned title every title-slot fill produces.
const CANNED_TITLE: &str = "The Ashen Threshold";
/// The canned name every name-slot fill produces.
const CANNED_NAME: &str = "Riverwatch";
/// The canned description every description-slot fill produces.
const CANNED_DESCRIPTION: &str = "A windswept keep where the river forks under a bruised sky.";
/// A ship-namespace source in the embedded ledger, used to prove grounding
/// provenance names its source. Coupled to that ledger entry existing.
const TEST_SOURCE_ID: &str = "polti";

/// A scripted structured backend: returns canned prose shaped to the requested
/// schema (a `TextProse` for the title, a `NamedProse` otherwise). Deterministic
/// and infinite — every call succeeds — so a whole world can be filled from it.
#[derive(Debug)]
struct CannedBackend;

#[async_trait]
impl StructuredBackend for CannedBackend {
    async fn complete_json(
        &self,
        _prompt: &str,
        schema_name: &str,
        _schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError> {
        let value = if schema_name == TITLE_SCHEMA {
            json!({ "text": CANNED_TITLE })
        } else {
            json!({ "name": CANNED_NAME, "description": CANNED_DESCRIPTION })
        };
        Ok((value, None))
    }

    async fn complete_text(&self, _prompt: &str) -> Result<(String, Option<Usage>), BackendError> {
        Err(BackendError::new(
            "text completion is not used by the content passes",
        ))
    }
}

fn test_llm() -> Llm {
    Llm::new(Box::new(CannedBackend))
}

#[tokio::test]
async fn fill_content_fills_every_targeted_slot_with_llm_provenance() {
    let generator = Generator::with_default_passes();
    let mut world = generator
        .generate_structure(SEED)
        .expect("the default pipeline generates a world");
    let llm = test_llm();
    let knowledge = test_knowledge();
    let context = ContentContext {
        llm: &llm,
        knowledge: &knowledge,
        model: MODEL_LABEL,
        config: &ContentConfig::default(),
    };

    generator
        .fill_content(&mut world, &context)
        .await
        .expect("content fill succeeds against the canned backend");

    assert_all_slots_filled_with_llm_provenance(&world, MODEL_LABEL);

    assert_eq!(
        world.meta.title.value().map(String::as_str),
        Some(CANNED_TITLE)
    );
    let first_location = world
        .locations
        .values()
        .next()
        .expect("at least one location");
    assert_eq!(
        first_location.name.value().map(String::as_str),
        Some(CANNED_NAME)
    );
    assert_eq!(
        first_location.description.value().map(String::as_str),
        Some(CANNED_DESCRIPTION),
    );
}

/// The structural facts the content phase must never touch.
#[derive(Debug, PartialEq, Eq, Serialize)]
struct EntityStructure {
    role: Option<ScoredOne<ProppRole>>,
    archetype: Option<ScoredOne<Archetype>>,
    location: Option<LocationId>,
}

/// A byte-comparable fingerprint of everything structural: location keys + exits +
/// occupancy, entity keys + role/archetype/location, item keys, the narrative
/// spine's stage sequence, the cursor's stage, and the player's location.
#[derive(Debug, PartialEq, Eq, Serialize)]
struct StructuralFingerprint {
    locations: BTreeMap<u64, Vec<(Direction, LocationId)>>,
    location_entities: BTreeMap<u64, BTreeSet<EntityId>>,
    location_items: BTreeMap<u64, BTreeSet<ItemId>>,
    entities: BTreeMap<u64, EntityStructure>,
    items: Vec<ItemId>,
    spine: Vec<MonomythStage>,
    cursor_stage: Option<MonomythStage>,
    player_location: LocationId,
}

/// A stable ordinal key, avoiding a lossy `as` cast.
fn ordinal(index: usize) -> u64 {
    u64::try_from(index).expect("world element counts fit in u64")
}

/// Index locations and entities by their stable insertion order so the fingerprint
/// is keyed by something ordered independent of the slotmap key's serialized shape.
fn fingerprint(world: &World) -> StructuralFingerprint {
    let mut locations = BTreeMap::new();
    let mut location_entities = BTreeMap::new();
    let mut location_items = BTreeMap::new();
    for (index, location) in world.locations.values().enumerate() {
        let key = ordinal(index);
        locations.insert(key, location.exits.iter().map(|(&d, &l)| (d, l)).collect());
        location_entities.insert(key, location.entities.clone());
        location_items.insert(key, location.items.clone());
    }

    let entities = world
        .entities
        .values()
        .enumerate()
        .map(|(index, entity)| {
            (
                ordinal(index),
                EntityStructure {
                    role: entity.role.clone(),
                    archetype: entity.archetype.clone(),
                    location: entity.location,
                },
            )
        })
        .collect();

    let structure = &world.story.structure;
    StructuralFingerprint {
        locations,
        location_entities,
        location_items,
        entities,
        items: world.items.keys().collect(),
        spine: structure
            .spine()
            .into_iter()
            .filter_map(|id| structure.node(id).map(|node| *node.stage.primary()))
            .collect(),
        cursor_stage: structure
            .node(world.state.cursor)
            .map(|node| *node.stage.primary()),
        player_location: world.player.location,
    }
}

#[tokio::test]
async fn fill_content_changes_only_content_never_structure() {
    let generator = Generator::with_default_passes();
    let mut world = generator
        .generate_structure(SEED)
        .expect("the default pipeline generates a world");

    let before = serde_json::to_string(&fingerprint(&world)).expect("fingerprint serializes");

    let llm = test_llm();
    let knowledge = test_knowledge();
    let context = ContentContext {
        llm: &llm,
        knowledge: &knowledge,
        model: MODEL_LABEL,
        config: &ContentConfig::default(),
    };
    generator
        .fill_content(&mut world, &context)
        .await
        .expect("content fill succeeds");

    let after = serde_json::to_string(&fingerprint(&world)).expect("fingerprint serializes");
    assert_eq!(
        before, after,
        "the content phase must not alter any structure"
    );
}

#[tokio::test]
async fn fill_content_grounds_provenance_on_an_ingested_ship_source() {
    let generator = Generator::with_default_passes();
    let mut world = generator
        .generate_structure(SEED)
        .expect("the default pipeline generates a world");

    let knowledge = test_knowledge();
    knowledge
        .ingest(
            TEST_SOURCE_ID,
            IngestInput::new("The Suppliant implores a Power in authority."),
        )
        .await
        .expect("a ship source ingests");

    let llm = test_llm();
    let context = ContentContext {
        llm: &llm,
        knowledge: &knowledge,
        model: MODEL_LABEL,
        config: &ContentConfig::default(),
    };
    generator
        .fill_content(&mut world, &context)
        .await
        .expect("content fill succeeds with grounding");

    let notes = world
        .meta
        .title
        .provenance()
        .expect("title is filled")
        .notes
        .clone();
    assert!(
        notes.contains(TEST_SOURCE_ID),
        "provenance notes must name the grounding source, got: {notes}",
    );
}
