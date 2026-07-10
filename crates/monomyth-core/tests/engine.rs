//! Behavioral tests over the public contract of `monomyth-core`: serde stability,
//! RNG determinism, the engine's happy and error paths, full-session replay, and
//! the frameworks binding.

use std::collections::{BTreeMap, BTreeSet};

use monomyth_core::{
    Action, ActionError, Content, ContentKind, ContentPrompt, Direction, EdgeKind, Entity,
    EntityId, EntityKind, Event, ExamineTarget, Item, ItemId, Location, LocationId, NarrativeEdit,
    NarrativeNode, NarrativeNodeId, NarrativeStructure, NodeKind, NodeSpec, Player, Provenance,
    Quest, RngState, SCHEMA_VERSION, Story, World, WorldMeta, WorldState, apply,
};
use monomyth_frameworks::{MonomythStage, PoltiSituation, arc_functions};
use slotmap::SlotMap;

/// The typed ids of the elements a [`fixture`] world contains, for assertions.
struct Fixture {
    world: World,
    start: LocationId,
    hall: LocationId,
    torch: ItemId,
    altar: ItemId,
    sphinx: EntityId,
}

/// A description slot filled with an exact value, so `Examine` text is assertable.
fn described(value: &str) -> Content {
    Content::filled(
        value.to_string(),
        ContentPrompt::new(ContentKind::Description, "a description"),
        Provenance::procedural("fixture"),
    )
}

/// A name slot filled with an exact value.
fn named(value: &str) -> Content {
    Content::filled(
        value.to_string(),
        ContentPrompt::new(ContentKind::Name, "a name"),
        Provenance::procedural("fixture"),
    )
}

/// Build a fixed two-room world from `seed` for deterministic tests.
///
/// `start` holds a portable `torch` and a fixed `altar` and a `sphinx` entity,
/// and has a north exit into `hall`.
fn fixture(seed: u64) -> Fixture {
    let mut locations = SlotMap::with_key();
    let mut entities = SlotMap::with_key();
    let mut items = SlotMap::with_key();

    let torch = items.insert(Item {
        name: named("torch"),
        description: described("a guttering pine torch"),
        portable: true,
    });
    let altar = items.insert(Item {
        name: named("altar"),
        description: described("a basalt altar bolted to the floor"),
        portable: false,
    });
    let sphinx = entities.insert(Entity {
        name: named("sphinx"),
        description: described("a riddling sphinx with folded wings"),
        kind: EntityKind::Creature,
        role: Some(monomyth_frameworks::ProppRole::Villain),
        archetype: Some(monomyth_frameworks::Archetype::ThresholdGuardian),
        location: None,
    });

    let start = locations.insert(Location {
        name: named("threshold"),
        description: described("a windswept threshold of cracked flagstones"),
        exits: BTreeMap::new(),
        entities: BTreeSet::from([sphinx]),
        items: BTreeSet::from([torch, altar]),
    });
    let hall = locations.insert(Location {
        name: named("hall"),
        description: described("a colonnaded hall receding into dark"),
        exits: BTreeMap::new(),
        entities: BTreeSet::new(),
        items: BTreeSet::new(),
    });
    locations[start].exits.insert(Direction::North, hall);
    locations[hall].exits.insert(Direction::South, start);
    if let Some(entity) = entities.get_mut(sphinx) {
        entity.location = Some(start);
    }

    let mut nodes = SlotMap::with_key();
    let root = nodes.insert(NarrativeNode::new(
        "CallToAdventure",
        NodeKind::Ending,
        MonomythStage::CallToAdventure,
        Content::empty(ContentPrompt::new(ContentKind::Synopsis, "the call")),
    ));
    let structure = NarrativeStructure {
        nodes,
        root,
        endings: BTreeSet::from([root]),
    };
    let story = Story {
        structure,
        plot: None,
        quests: {
            let mut quests = SlotMap::with_key();
            quests.insert(Quest {
                title: named("Answer the Sphinx"),
                situation: Some(PoltiSituation::TheEnigma),
                complete: false,
            });
            quests
        },
    };

    let world = World {
        meta: WorldMeta {
            seed,
            schema_version: SCHEMA_VERSION,
            title: named("The Threshold"),
        },
        locations,
        entities,
        items,
        player: Player::new(start),
        story,
        state: WorldState {
            cursor: root,
            ..WorldState::default()
        },
        rng: RngState::new(seed),
    };

    Fixture {
        world,
        start,
        hall,
        torch,
        altar,
        sphinx,
    }
}

#[test]
fn should_round_trip_world_to_byte_identical_json() {
    let world = fixture(99).world;
    let first = serde_json::to_string(&world).expect("serialize world");
    let restored: World = serde_json::from_str(&first).expect("deserialize world");
    let second = serde_json::to_string(&restored).expect("re-serialize world");
    assert_eq!(
        first, second,
        "serialized world must be byte-stable across a round-trip"
    );
}

#[test]
fn should_serialize_enums_as_variant_name_strings() {
    let json = serde_json::to_string(&Direction::North).expect("serialize direction");
    assert_eq!(json, "\"North\"");
    let stage = serde_json::to_string(&MonomythStage::CallToAdventure).expect("serialize stage");
    assert_eq!(stage, "\"CallToAdventure\"");
}

#[test]
fn same_seed_rng_states_produce_identical_sequences() {
    let mut a = RngState::new(2024);
    let mut b = RngState::new(2024);
    let draws_a: Vec<u64> = (0..8).map(|_| a.next_u64()).collect();
    let draws_b: Vec<u64> = (0..8).map(|_| b.next_u64()).collect();
    assert_eq!(draws_a, draws_b);
    assert_eq!(a.word_pos(), b.word_pos());
}

#[test]
fn rng_continues_same_sequence_after_serde_round_trip() {
    let mut live = RngState::new(2024);
    let mut reference = RngState::new(2024);
    for _ in 0..5 {
        let _ = live.next_u64();
        let _ = reference.next_u64();
    }
    let encoded = serde_json::to_string(&live).expect("serialize rng");
    let mut restored: RngState = serde_json::from_str(&encoded).expect("deserialize rng");
    let next_reference: Vec<u64> = (0..4).map(|_| reference.next_u64()).collect();
    let next_restored: Vec<u64> = (0..4).map(|_| restored.next_u64()).collect();
    assert_eq!(
        next_reference, next_restored,
        "a restored rng must continue the exact sequence"
    );
}

#[test]
fn should_move_player_through_valid_exit() {
    let Fixture {
        mut world,
        start,
        hall,
        ..
    } = fixture(1);
    let events = apply(&mut world, Action::Move(Direction::North)).expect("move succeeds");
    assert_eq!(
        events,
        vec![Event::Moved {
            from: start,
            to: hall
        }]
    );
    assert_eq!(world.player.location, hall);
    assert_eq!(world.state.turn, 1);
}

#[test]
fn move_returns_no_exit_for_absent_direction() {
    let Fixture {
        mut world, start, ..
    } = fixture(1);
    let error = apply(&mut world, Action::Move(Direction::West)).expect_err("no west exit");
    assert_eq!(error, ActionError::NoExit(Direction::West));
    assert_eq!(
        world.player.location, start,
        "a failed move must not relocate the player"
    );
    assert_eq!(
        world.state.turn, 0,
        "a failed action must not consume a turn"
    );
}

#[test]
fn should_take_portable_item_present_in_room() {
    let Fixture {
        mut world,
        start,
        torch,
        ..
    } = fixture(1);
    let events = apply(&mut world, Action::Take(torch)).expect("take succeeds");
    assert_eq!(events, vec![Event::Took(torch)]);
    assert!(world.player.inventory.contains(&torch));
    assert!(!world.locations[start].items.contains(&torch));
}

#[test]
fn take_returns_not_portable_for_fixed_item() {
    let Fixture {
        mut world, altar, ..
    } = fixture(1);
    let error = apply(&mut world, Action::Take(altar)).expect_err("altar is fixed");
    assert_eq!(error, ActionError::NotPortable(altar));
    assert!(!world.player.inventory.contains(&altar));
}

#[test]
fn take_returns_item_not_here_for_item_in_other_room() {
    let mut fixture = fixture(1);
    fixture.world.locations[fixture.start]
        .items
        .remove(&fixture.torch);
    fixture.world.locations[fixture.hall]
        .items
        .insert(fixture.torch);
    let error =
        apply(&mut fixture.world, Action::Take(fixture.torch)).expect_err("torch elsewhere");
    assert_eq!(error, ActionError::ItemNotHere(fixture.torch));
}

#[test]
fn should_drop_carried_item_into_current_room() {
    let Fixture {
        mut world,
        start,
        torch,
        ..
    } = fixture(1);
    apply(&mut world, Action::Take(torch)).expect("take first");
    let events = apply(&mut world, Action::Drop(torch)).expect("drop succeeds");
    assert_eq!(events, vec![Event::Dropped(torch)]);
    assert!(!world.player.inventory.contains(&torch));
    assert!(world.locations[start].items.contains(&torch));
    assert_eq!(world.state.turn, 2);
}

#[test]
fn drop_returns_not_in_inventory_for_unheld_item() {
    let Fixture {
        mut world, torch, ..
    } = fixture(1);
    let error = apply(&mut world, Action::Drop(torch)).expect_err("torch not held");
    assert_eq!(error, ActionError::ItemNotInInventory(torch));
}

#[test]
fn examine_reports_exact_filled_descriptions() {
    let Fixture {
        mut world,
        torch,
        sphinx,
        ..
    } = fixture(1);
    let room = apply(&mut world, Action::Examine(ExamineTarget::Location)).expect("examine room");
    assert_eq!(
        room,
        vec![Event::Examined {
            text: "a windswept threshold of cracked flagstones".to_string()
        }]
    );
    let item =
        apply(&mut world, Action::Examine(ExamineTarget::Item(torch))).expect("examine item");
    assert_eq!(
        item,
        vec![Event::Examined {
            text: "a guttering pine torch".to_string()
        }]
    );
    let being =
        apply(&mut world, Action::Examine(ExamineTarget::Entity(sphinx))).expect("examine entity");
    assert_eq!(
        being,
        vec![Event::Examined {
            text: "a riddling sphinx with folded wings".to_string()
        }]
    );
}

#[test]
fn identical_action_sequences_yield_identical_events_and_worlds() {
    let run = |seed: u64| -> (Vec<Event>, String) {
        let Fixture {
            mut world, torch, ..
        } = fixture(seed);
        let actions = [
            Action::Examine(ExamineTarget::Location),
            Action::Take(torch),
            Action::Move(Direction::North),
            Action::Drop(torch),
            Action::Wait,
        ];
        let mut events = Vec::new();
        for action in actions {
            events.extend(apply(&mut world, action).expect("scripted action applies"));
        }
        let json = serde_json::to_string(&world).expect("serialize replayed world");
        (events, json)
    };

    let (events_a, json_a) = run(7);
    let (events_b, json_b) = run(7);
    assert_eq!(
        events_a, events_b,
        "same seed + script must yield identical events"
    );
    assert_eq!(
        json_a, json_b,
        "same seed + script must yield byte-identical worlds"
    );
}

#[test]
fn narrative_node_arc_functions_match_crosswalk() {
    let stage = MonomythStage::TheRoadOfTrials;
    let node = NarrativeNode::new(
        "TheRoadOfTrials",
        NodeKind::Beat,
        stage,
        Content::empty(ContentPrompt::new(ContentKind::Synopsis, "trials")),
    );
    assert_eq!(node.arc_functions(), arc_functions(stage));
}

#[test]
fn move_returns_unknown_location_for_stale_player_location() {
    let Fixture {
        mut world, start, ..
    } = fixture(1);
    world.locations.remove(start);
    let error = apply(&mut world, Action::Move(Direction::North))
        .expect_err("stale player location must fail");
    assert_eq!(error, ActionError::UnknownLocation(start));
}

#[test]
fn move_returns_unknown_location_for_exit_to_missing_destination() {
    let Fixture {
        mut world, hall, ..
    } = fixture(1);
    world.locations.remove(hall);
    let error = apply(&mut world, Action::Move(Direction::North))
        .expect_err("exit to missing destination must fail");
    assert_eq!(error, ActionError::UnknownLocation(hall));
}

#[test]
fn take_returns_unknown_item_for_id_absent_from_world_items() {
    let Fixture {
        mut world,
        start,
        torch,
        ..
    } = fixture(1);
    world.items.remove(torch);
    assert!(world.locations[start].items.contains(&torch));
    let error = apply(&mut world, Action::Take(torch)).expect_err("unknown item id must fail");
    assert_eq!(error, ActionError::UnknownItem(torch));
}

#[test]
fn examine_returns_unknown_entity_for_unknown_id() {
    let Fixture {
        mut world, sphinx, ..
    } = fixture(1);
    world.entities.remove(sphinx);
    let error = apply(&mut world, Action::Examine(ExamineTarget::Entity(sphinx)))
        .expect_err("unknown entity id must fail");
    assert_eq!(error, ActionError::UnknownEntity(sphinx));
}

#[test]
fn examine_returns_unknown_item_for_unknown_id() {
    let Fixture {
        mut world, torch, ..
    } = fixture(1);
    world.items.remove(torch);
    let error = apply(&mut world, Action::Examine(ExamineTarget::Item(torch)))
        .expect_err("unknown item id must fail");
    assert_eq!(error, ActionError::UnknownItem(torch));
}

#[test]
fn examine_falls_back_to_prompt_hint_for_empty_description() {
    let Fixture {
        mut world, start, ..
    } = fixture(1);
    let hint = "an unlit chamber awaiting its description";
    world.locations[start].description =
        Content::empty(ContentPrompt::new(ContentKind::Description, hint));
    let events = apply(&mut world, Action::Examine(ExamineTarget::Location))
        .expect("examine empty description");
    assert_eq!(
        events,
        vec![Event::Examined {
            text: hint.to_string()
        }],
        "an empty slot must surface its prompt hint verbatim"
    );
}

/// A world whose narrative cursor sits on a fork: `root =(Sequence)=> a` and
/// `root =(Choice)=> b`, with `a` and `b` distinct endings. Returns the world and
/// the three node ids.
fn forked_world() -> (World, NarrativeNodeId, NarrativeNodeId, NarrativeNodeId) {
    let mut world = fixture(1).world;
    let structure = &mut world.story.structure;
    let root = structure.root();
    let spec_a = NodeSpec::new("A", MonomythStage::TheRoadOfTrials, "branch a");
    let spec_b = NodeSpec::new("B", MonomythStage::TheRoadOfTrials, "branch b");
    let Ok(monomyth_core::EditOutcome::NodeAdded(a)) =
        structure.apply_edit(&NarrativeEdit::AddNode { spec: spec_a })
    else {
        panic!("AddNode reports the new id");
    };
    let Ok(monomyth_core::EditOutcome::NodeAdded(b)) =
        structure.apply_edit(&NarrativeEdit::AddNode { spec: spec_b })
    else {
        panic!("AddNode reports the new id");
    };
    structure
        .apply_edits(&[
            NarrativeEdit::Connect {
                source: root,
                target: a,
                kind: EdgeKind::Sequence,
            },
            NarrativeEdit::Connect {
                source: root,
                target: b,
                kind: EdgeKind::Choice,
            },
            NarrativeEdit::MarkEnding { node: a },
            NarrativeEdit::MarkEnding { node: b },
            NarrativeEdit::UnmarkEnding { node: root },
        ])
        .expect("the fork is a valid structure");
    world.state.cursor = root;
    (world, root, a, b)
}

#[test]
fn choose_advances_the_cursor_along_a_branch() {
    let (mut world, root, _a, b) = forked_world();
    let events = apply(&mut world, Action::Choose(b)).expect("choosing an open branch succeeds");
    assert_eq!(events, vec![Event::Advanced { from: root, to: b }]);
    assert_eq!(world.state.cursor, b, "the cursor moves to the chosen beat");
    assert_eq!(world.state.turn, 1, "a successful choice spends a turn");
}

#[test]
fn choose_rejects_a_node_that_is_not_a_branch() {
    let (mut world, root, _a, _b) = forked_world();
    let error =
        apply(&mut world, Action::Choose(root)).expect_err("root is not a branch of itself");
    assert_eq!(error, ActionError::NotAChoice(root));
    assert_eq!(
        world.state.cursor, root,
        "a failed choice leaves the cursor put"
    );
    assert_eq!(world.state.turn, 0, "a failed choice spends no turn");
}

#[test]
fn choose_is_blocked_by_an_unmet_guard() {
    let (mut world, root, _a, b) = forked_world();
    world
        .story
        .structure
        .apply_edits(&[NarrativeEdit::SetEdgeGuard {
            source: root,
            target: b,
            guard: Some("gate_open".to_owned()),
        }])
        .expect("guarding an edge keeps the structure valid");

    let blocked = apply(&mut world, Action::Choose(b)).expect_err("a gated branch is blocked");
    assert_eq!(blocked, ActionError::ChoiceBlocked(b));
    assert_eq!(world.state.cursor, root);

    world.state.flags.insert("gate_open".to_owned(), true);
    let events =
        apply(&mut world, Action::Choose(b)).expect("the branch opens once the flag is set");
    assert_eq!(events, vec![Event::Advanced { from: root, to: b }]);
    assert_eq!(world.state.cursor, b);
}

#[test]
fn choose_at_an_ending_has_no_available_branch() {
    let (mut world, root, a, b) = forked_world();
    apply(&mut world, Action::Choose(a)).expect("advance to an ending");
    let error = apply(&mut world, Action::Choose(b)).expect_err("an ending offers no choices");
    assert_eq!(error, ActionError::NotAChoice(b));
    let _ = root;
}
