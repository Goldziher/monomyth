//! A minimal, structured [`Renderer`] implementation over the monomyth domain
//! model (ADR-0020, the P-RENDER plane).
//!
//! `monomyth-render-terse` exists to prove the P-RENDER seam: it is a second,
//! independent [`Renderer`] impl selected purely by config
//! (`[render].medium = "terse"`) alongside `monomyth-text`'s prose renderer, with
//! no shared code between the two beyond the trait and `monomyth-core`. Its
//! output favors compact `label: value` lines over prose — there is no flourish,
//! no resolved sentence structure, just the facts a caller needs to drive a
//! session — so its output is trivially distinguishable from `monomyth-text`'s.
//!
//! Like `monomyth-text`, this crate depends on `monomyth-core` and
//! `monomyth-contracts` only; it never depends on another renderer crate, and an
//! empty [`Content`] slot always renders as a bracketed placeholder rather than
//! panicking.

#![forbid(unsafe_code)]

use std::fmt::Write as _;

use monomyth_contracts::Renderer;
use monomyth_core::{
    Content, EdgeKind, Event, LocationId, NarrativeNodeId, NarrativeStructure, World,
};

/// Placeholder for a slot whose value and prompt hint are both absent.
const UNNAMED_PLACEHOLDER: &str = "[unnamed]";
/// Placeholder for a location id that does not resolve in the world.
const MISSING_LOCATION: &str = "[unknown location]";

/// A minimal, structured [`Renderer`]: compact `label: value` lines, no prose.
///
/// The only job of this type is to exist as a second medium the composition root
/// can select by config, so `[render].medium` genuinely branches between two
/// independent implementations rather than one implementation under two names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerseRenderer;

impl Renderer for TerseRenderer {
    fn intro(&self, world: &World) -> String {
        format!(
            "title: {}\nseed: {}",
            slot_text(&world.meta.title),
            world.meta.seed
        )
    }

    fn location(&self, world: &World) -> String {
        let Some(location) = world.locations.get(world.player.location) else {
            return format!("location: {MISSING_LOCATION}");
        };

        let mut out = format!(
            "location: {}\ndesc: {}\nexits: {}",
            slot_text(&location.name),
            slot_text(&location.description),
            exits_line(location.exits.keys().copied()),
        );
        if !location.items.is_empty() {
            let names = location
                .items
                .iter()
                .map(|&id| item_name(world, id))
                .collect::<Vec<_>>()
                .join(",");
            let _ = write!(out, "\nitems: {names}");
        }
        if !location.entities.is_empty() {
            let names = location
                .entities
                .iter()
                .map(|&id| entity_name(world, id))
                .collect::<Vec<_>>()
                .join(",");
            let _ = write!(out, "\nentities: {names}");
        }
        out
    }

    fn event(&self, event: &Event, world: &World) -> String {
        match event {
            Event::Moved { to, .. } => format!("event=move to={}", location_name(world, *to)),
            Event::Took(item) => format!("event=take item={}", item_name(world, *item)),
            Event::Dropped(item) => format!("event=drop item={}", item_name(world, *item)),
            Event::Examined { text } => format!("event=examine text={text}"),
            Event::Waited => String::from("event=wait"),
            Event::Advanced { to, .. } => {
                format!(
                    "event=advance to={}",
                    stage_label(&world.story.structure, *to)
                )
            }
            // `Event` is `#[non_exhaustive]`, so a future variant must degrade gracefully.
            _ => String::from("event=unknown"),
        }
    }

    fn events(&self, events: &[Event], world: &World) -> String {
        events
            .iter()
            .map(|event| self.event(event, world))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn structure(&self, world: &World) -> String {
        let structure = &world.story.structure;
        let plot = world
            .story
            .plot
            .as_ref()
            .map(monomyth_core::ScoredOne::primary);
        let spine = structure
            .spine()
            .into_iter()
            .map(|id| stage_label(structure, id))
            .collect::<Vec<_>>()
            .join(",");
        let endings = structure
            .endings()
            .map(|id| stage_label(structure, id))
            .collect::<Vec<_>>()
            .join(",");
        format!("plot: {plot:?}\nspine: {spine}\nendings: {endings}")
    }

    fn choices(&self, world: &World) -> String {
        let structure = &world.story.structure;
        let choices = structure.available_choices(world.state.cursor, &world.state.flags);
        if choices.is_empty() {
            return String::from("choices: none");
        }
        let entries = choices
            .iter()
            .enumerate()
            .map(|(index, edge)| {
                format!(
                    "{}={}:{}",
                    index + 1,
                    edge_kind_label(edge.kind),
                    stage_label(structure, edge.target)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("choices: {entries}")
    }
}

/// The lowercase name of an [`EdgeKind`], for a terse choice entry.
const fn edge_kind_label(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Sequence => "continue",
        EdgeKind::Choice => "choose",
        EdgeKind::Fork => "branch",
    }
}

/// The Campbell stage name of a node, or a placeholder if the id does not resolve.
fn stage_label(structure: &NarrativeStructure, id: NarrativeNodeId) -> String {
    structure.node(id).map_or_else(
        || String::from("[?]"),
        |node| format!("{:?}", node.stage.primary()),
    )
}

/// The filled value of a slot, or a bracketed placeholder while it is still empty.
///
/// An empty slot with a prompt hint renders as `[hint]`; an empty slot with a blank
/// hint renders as [`UNNAMED_PLACEHOLDER`]. Never panics.
fn slot_text(content: &Content) -> String {
    if let Some(value) = content.value() {
        return value.clone();
    }
    let hint = &content.prompt().hint;
    if hint.is_empty() {
        UNNAMED_PLACEHOLDER.to_owned()
    } else {
        format!("[{hint}]")
    }
}

/// The comma-separated exit directions of a location, or `none` when it has none.
fn exits_line(directions: impl Iterator<Item = monomyth_core::Direction>) -> String {
    let names: Vec<&str> = directions.map(direction_name).collect();
    if names.is_empty() {
        String::from("none")
    } else {
        names.join(",")
    }
}

/// The lowercase name of a compass or vertical direction.
const fn direction_name(direction: monomyth_core::Direction) -> &'static str {
    use monomyth_core::Direction;
    match direction {
        Direction::North => "north",
        Direction::South => "south",
        Direction::East => "east",
        Direction::West => "west",
        Direction::Up => "up",
        Direction::Down => "down",
    }
}

/// The name of a location by id, or a placeholder if the id does not resolve.
fn location_name(world: &World, id: LocationId) -> String {
    world.locations.get(id).map_or_else(
        || MISSING_LOCATION.to_owned(),
        |location| slot_text(&location.name),
    )
}

/// The name of an item by id, or a placeholder if the id does not resolve.
fn item_name(world: &World, id: monomyth_core::ItemId) -> String {
    world
        .items
        .get(id)
        .map_or_else(|| "[unknown item]".to_owned(), |item| slot_text(&item.name))
}

/// The name of an entity by id, or a placeholder if the id does not resolve.
fn entity_name(world: &World, id: monomyth_core::EntityId) -> String {
    world.entities.get(id).map_or_else(
        || "[unknown entity]".to_owned(),
        |entity| slot_text(&entity.name),
    )
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::collections::BTreeSet;

    use monomyth_core::doc_support::single_room_world;
    use monomyth_core::{
        Content, ContentKind, ContentPrompt, Direction, Entity, EntityKind, Event, Item, Location,
        Provenance, World,
    };

    use super::{Renderer, TerseRenderer};

    /// A filled content slot carrying `value`, for building test worlds.
    fn filled(kind: ContentKind, value: &str) -> Content {
        Content::filled(
            value.to_string(),
            ContentPrompt::new(kind, "hint"),
            Provenance::procedural("test"),
        )
    }

    /// A one-room world holding a filled location, a north exit, an item, and an
    /// entity, so the full render path is exercised.
    fn filled_world() -> World {
        let mut world = single_room_world();
        let room = world.player.location;

        let hall = world.locations.insert(Location {
            name: filled(ContentKind::Name, "Great Hall"),
            description: filled(ContentKind::Description, "A vast echoing hall."),
            exits: BTreeMap::new(),
            entities: BTreeSet::new(),
            items: BTreeSet::new(),
        });
        let sword = world.items.insert(Item {
            name: filled(ContentKind::Name, "iron sword"),
            description: filled(ContentKind::Description, "A pitted blade."),
            portable: true,
        });
        let sphinx = world.entities.insert(Entity {
            name: filled(ContentKind::Name, "sphinx"),
            description: filled(ContentKind::Description, "It watches."),
            kind: EntityKind::Creature,
            role: None,
            archetype: None,
            location: Some(room),
        });

        let location = world.locations.get_mut(room).expect("room exists");
        location.name = filled(ContentKind::Name, "Antechamber");
        location.description = filled(ContentKind::Description, "A bare stone room.");
        location.exits.insert(Direction::North, hall);
        location.items.insert(sword);
        location.entities.insert(sphinx);
        world
    }

    #[test]
    fn should_render_terse_intro() {
        let world = single_room_world();
        assert_eq!(
            TerseRenderer.intro(&world),
            "title: [an untitled world]\nseed: 0"
        );
    }

    #[test]
    fn should_render_placeholder_for_unfilled_location() {
        let world = single_room_world();
        assert_eq!(
            TerseRenderer.location(&world),
            "location: [the antechamber]\ndesc: [a bare stone room]\nexits: none",
        );
    }

    #[test]
    fn should_render_filled_location_with_exits_items_and_entities() {
        let world = filled_world();
        assert_eq!(
            TerseRenderer.location(&world),
            "location: Antechamber\ndesc: A bare stone room.\nexits: north\nitems: iron sword\nentities: sphinx",
        );
    }

    #[test]
    fn should_render_waited_event() {
        let world = single_room_world();
        assert_eq!(TerseRenderer.event(&Event::Waited, &world), "event=wait");
    }

    #[test]
    fn should_render_moved_event_with_destination_name() {
        let world = filled_world();
        let to = world
            .locations
            .keys()
            .find(|&id| id != world.player.location)
            .expect("a second location exists");
        let event = Event::Moved {
            from: world.player.location,
            to,
        };
        assert_eq!(
            TerseRenderer.event(&event, &world),
            "event=move to=Great Hall"
        );
    }

    #[test]
    fn should_join_multiple_events_with_newlines() {
        let world = single_room_world();
        let events = [Event::Waited, Event::Waited];
        assert_eq!(
            TerseRenderer.events(&events, &world),
            "event=wait\nevent=wait"
        );
    }

    #[test]
    fn should_render_choices_as_none_at_an_ending() {
        let world = single_room_world();
        assert_eq!(TerseRenderer.choices(&world), "choices: none");
    }

    #[test]
    fn output_is_stable_across_calls() {
        let world = filled_world();
        assert_eq!(
            TerseRenderer.location(&world),
            TerseRenderer.location(&world)
        );
        assert_eq!(TerseRenderer.intro(&world), TerseRenderer.intro(&world));
    }

    #[test]
    fn terse_output_differs_from_prose_output() {
        let world = filled_world();
        let terse_location = TerseRenderer.location(&world);
        // monomyth-text's render_location for the same world reads ~keep
        // "Antechamber\nA bare stone room.\n\nExits: north\nItems here: iron ~keep
        // sword\nYou see: sphinx" — asserted independently in that crate's own ~keep
        // tests (this crate never depends on monomyth-text). The terse format's ~keep
        // `label: value` shape and lack of prose connectives make the two ~keep
        // trivially distinguishable without needing that dependency. ~keep
        assert!(terse_location.starts_with("location: Antechamber\ndesc: "));
        assert!(!terse_location.contains("You see:"));
        assert!(!terse_location.contains("Exits:"));
    }
}
