//! Behavioural tests over the whole-[`World`] validation contract and the
//! schema-guarded load: every [`WorldError`] variant from a targeted mutation of a
//! valid fixture, and the three [`LoadError`] rejection paths.

use monomyth_core::doc_support::single_room_world;
use monomyth_core::{
    Content, ContentKind, ContentPrompt, Entity, EntityId, EntityKind, Item, ItemId, LoadError,
    LocationId, NarrativeError, NarrativeNodeId, SCHEMA_VERSION, World, WorldError,
};

/// An empty name slot hinted with `hint`.
fn name(hint: &str) -> Content {
    Content::empty(ContentPrompt::new(ContentKind::Name, hint))
}

/// A minimal entity placed at `location`.
fn entity(location: Option<LocationId>) -> Entity {
    Entity {
        name: name("a figure"),
        description: name("a shadowed figure"),
        kind: EntityKind::Npc,
        role: None,
        archetype: None,
        location,
    }
}

/// A minimal portable item.
fn item() -> Item {
    Item {
        name: name("a key"),
        description: name("a brass key"),
        portable: true,
    }
}

/// The single-room world plus one entity and one item both placed in the room,
/// with both sides of every relationship in sync — a valid multi-element fixture.
fn populated_world() -> (World, LocationId, EntityId, ItemId) {
    let mut world = single_room_world();
    let room = world.player.location;
    let entity_id = world.entities.insert(entity(Some(room)));
    let item_id = world.items.insert(item());
    world.locations[room].entities.insert(entity_id);
    world.locations[room].items.insert(item_id);
    (world, room, entity_id, item_id)
}

#[test]
fn should_accept_the_single_room_fixture() {
    assert_eq!(single_room_world().validate(), Ok(()));
}

#[test]
fn should_accept_a_populated_world() {
    let (world, _, _, _) = populated_world();
    assert_eq!(world.validate(), Ok(()));
}

#[test]
fn should_reject_dangling_player_location() {
    let mut world = single_room_world();
    world.player.location = LocationId::default();
    assert_eq!(
        world.validate(),
        Err(WorldError::DanglingPlayerLocation(LocationId::default()))
    );
}

#[test]
fn should_reject_dangling_inventory_item() {
    let (mut world, _, _, _) = populated_world();
    let orphan = world.items.insert(item());
    world.player.inventory.insert(orphan);
    world.items.remove(orphan);
    assert_eq!(
        world.validate(),
        Err(WorldError::DanglingInventoryItem(orphan))
    );
}

#[test]
fn should_reject_dangling_location_entity() {
    let (mut world, room, _, _) = populated_world();
    let orphan = world.entities.insert(entity(None));
    world.locations[room].entities.insert(orphan);
    world.entities.remove(orphan);
    assert_eq!(
        world.validate(),
        Err(WorldError::DanglingLocationEntity {
            location: room,
            entity: orphan,
        })
    );
}

#[test]
fn should_reject_dangling_location_item() {
    let (mut world, room, _, _) = populated_world();
    let orphan = world.items.insert(item());
    world.locations[room].items.insert(orphan);
    world.items.remove(orphan);
    assert_eq!(
        world.validate(),
        Err(WorldError::DanglingLocationItem {
            location: room,
            item: orphan,
        })
    );
}

#[test]
fn should_reject_dangling_entity_location() {
    let (mut world, _, _, _) = populated_world();
    let stray = world.entities.insert(entity(Some(LocationId::default())));
    assert_eq!(
        world.validate(),
        Err(WorldError::DanglingEntityLocation {
            entity: stray,
            location: LocationId::default(),
        })
    );
}

#[test]
fn should_reject_entity_not_mirrored_by_its_location() {
    let (mut world, room, _, _) = populated_world();
    let stray = world.entities.insert(entity(Some(room)));
    assert_eq!(
        world.validate(),
        Err(WorldError::EntityLocationDesync { entity: stray })
    );
}

#[test]
fn should_reject_location_listing_an_entity_that_points_elsewhere() {
    let (mut world, room, _, _) = populated_world();
    let stray = world.entities.insert(entity(None));
    world.locations[room].entities.insert(stray);
    assert_eq!(
        world.validate(),
        Err(WorldError::EntityLocationDesync { entity: stray })
    );
}

#[test]
fn should_reject_cursor_outside_the_structure() {
    let mut world = single_room_world();
    world.state.cursor = NarrativeNodeId::default();
    assert_eq!(
        world.validate(),
        Err(WorldError::CursorNotInStructure(NarrativeNodeId::default()))
    );
}

#[test]
fn should_surface_a_malformed_narrative_dag() {
    let mut world = single_room_world();
    world.story.structure.root = NarrativeNodeId::default();
    assert_eq!(
        world.validate(),
        Err(WorldError::Narrative(NarrativeError::MissingRoot(
            NarrativeNodeId::default()
        )))
    );
}

#[test]
fn should_round_trip_a_valid_world_through_from_json_checked() {
    let (world, _, _, item_id) = populated_world();
    let json = serde_json::to_string(&world).expect("serialize");
    let loaded = World::from_json_checked(&json).expect("load a valid world");
    assert_eq!(loaded.meta.schema_version, SCHEMA_VERSION);
    assert!(loaded.items.contains_key(item_id));
}

#[test]
fn should_reject_a_schema_version_mismatch() {
    let mut world = single_room_world();
    world.meta.schema_version = SCHEMA_VERSION + 1;
    let json = serde_json::to_string(&world).expect("serialize");
    let error = World::from_json_checked(&json).expect_err("bumped schema must be rejected");
    assert!(matches!(
        error,
        LoadError::SchemaMismatch { found, expected }
            if found == SCHEMA_VERSION + 1 && expected == SCHEMA_VERSION
    ));
}

#[test]
fn should_reject_malformed_json() {
    let error = World::from_json_checked("{ not a world }").expect_err("malformed json");
    assert!(matches!(error, LoadError::Deserialize(_)));
}

#[test]
fn should_reject_a_structurally_invalid_world() {
    let mut world = single_room_world();
    world.player.location = LocationId::default();
    let json = serde_json::to_string(&world).expect("serialize");
    let error = World::from_json_checked(&json).expect_err("invalid world");
    assert!(matches!(
        error,
        LoadError::Invalid(WorldError::DanglingPlayerLocation(location))
            if location == LocationId::default()
    ));
}
