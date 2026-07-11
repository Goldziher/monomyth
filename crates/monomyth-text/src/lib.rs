//! The text frontend: a pure, deterministic string renderer over the domain model.
//!
//! `monomyth-text` turns a [`World`] and its [`Event`]s into human-readable lines.
//! It is a frontend, so it depends on `monomyth-core` *only* — never on generation,
//! knowledge, or any LLM concern — and the serialized world is the sole contract it
//! consumes. Every function here is a pure function of its inputs: no IO, no runtime,
//! no terminal control. A richer TUI (ratatui, say) can layer on top later; this is
//! the tested rendering contract underneath it.
//!
//! # Unfilled content
//!
//! A world produced by the procedural pass *without* the content pass has every
//! [`Content`] slot [`Empty`](monomyth_core::Content::Empty). The renderer never
//! panics on such a world: an empty slot falls back to a deterministic placeholder
//! (its prompt hint in brackets, or `[unnamed]` when the hint itself is blank), so
//! structure is legible before any prose exists.
//!
//! # Example
//!
//! ```
//! use monomyth_core::doc_support::single_room_world;
//! use monomyth_text::render_intro;
//!
//! let world = single_room_world();
//! assert_eq!(render_intro(&world), "[an untitled world]\n\nSeed: 0");
//! ```

#![forbid(unsafe_code)]

use std::borrow::Cow;
use std::fmt::Write as _;

use monomyth_core::{
    Content, Direction, EdgeKind, EntityId, Event, ItemId, Location, LocationId, NarrativeNodeId,
    NarrativeStructure, World,
};

/// Placeholder for a slot whose value and prompt hint are both absent.
const UNNAMED_PLACEHOLDER: &str = "[unnamed]";
/// Placeholder for a location id that does not resolve in the world.
const MISSING_LOCATION: &str = "[unknown location]";
/// Placeholder for an item id that does not resolve in the world.
const MISSING_ITEM: &str = "[unknown item]";
/// Placeholder for an entity id that does not resolve in the world.
const MISSING_ENTITY: &str = "[unknown entity]";

/// Render the player's current location: its name, description, exits, items, and
/// the entities present, each resolved to a readable name.
///
/// Empty content slots surface a bracketed placeholder rather than panicking, so a
/// purely structural world still renders. The `Exits` line is always present
/// (`none` when there are none); the item and entity lines appear only when the
/// location holds something.
///
/// ```
/// use monomyth_core::doc_support::single_room_world;
/// use monomyth_text::render_location;
///
/// let world = single_room_world();
/// assert_eq!(
///     render_location(&world),
///     "[the antechamber]\n[a bare stone room]\n\nExits: none",
/// );
/// ```
#[must_use]
pub fn render_location(world: &World) -> String {
    let Some(location) = world.locations.get(world.player.location) else {
        return String::from(MISSING_LOCATION);
    };

    let mut lines = vec![
        slot_text(&location.name).into_owned(),
        slot_text(&location.description).into_owned(),
        String::new(),
        format!("Exits: {}", exits_line(location)),
    ];
    if !location.items.is_empty() {
        let names = location.items.iter().map(|&id| item_name(world, id));
        lines.push(format!("Items here: {}", comma_separated(names)));
    }
    if !location.entities.is_empty() {
        let names = location.entities.iter().map(|&id| entity_name(world, id));
        lines.push(format!("You see: {}", comma_separated(names)));
    }
    lines.join("\n")
}

/// Render a single [`Event`] as one human-readable line, resolving ids to names.
///
/// ```
/// use monomyth_core::doc_support::single_room_world;
/// use monomyth_core::{apply, Action};
/// use monomyth_text::render_event;
///
/// let mut world = single_room_world();
/// let events = apply(&mut world, Action::Wait)?;
/// assert_eq!(render_event(&events[0], &world), "You wait.");
/// # Ok::<(), monomyth_core::ActionError>(())
/// ```
#[must_use]
pub fn render_event(event: &Event, world: &World) -> String {
    match event {
        Event::Moved { to, .. } => format!("You travel to the {}.", location_name(world, *to)),
        Event::Took(item) => format!("You take the {}.", item_name(world, *item)),
        Event::Dropped(item) => format!("You drop the {}.", item_name(world, *item)),
        Event::Examined { text } => text.clone(),
        Event::Waited => String::from("You wait."),
        Event::Advanced { to, .. } => {
            format!(
                "The story moves on to {}.",
                stage_label(&world.story.structure, *to)
            )
        }
        // `Event` is `#[non_exhaustive]`, so a future variant must degrade gracefully
        _ => String::from("Something happens."),
    }
}

/// Render a slice of [`Event`]s as newline-joined lines, in order.
///
/// ```
/// use monomyth_core::doc_support::single_room_world;
/// use monomyth_core::{apply, Action};
/// use monomyth_text::render_events;
///
/// let mut world = single_room_world();
/// let mut events = apply(&mut world, Action::Wait)?;
/// events.extend(apply(&mut world, Action::Wait)?);
/// assert_eq!(render_events(&events, &world), "You wait.\nYou wait.");
/// # Ok::<(), monomyth_core::ActionError>(())
/// ```
#[must_use]
pub fn render_events(events: &[Event], world: &World) -> String {
    events
        .iter()
        .map(|event| render_event(event, world))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render the world's title and seed as an introduction banner.
///
/// ```
/// use monomyth_core::doc_support::single_room_world;
/// use monomyth_text::render_intro;
///
/// let world = single_room_world();
/// assert_eq!(render_intro(&world), "[an untitled world]\n\nSeed: 0");
/// ```
#[must_use]
pub fn render_intro(world: &World) -> String {
    format!(
        "{}\n\nSeed: {}",
        slot_text(&world.meta.title),
        world.meta.seed
    )
}

/// Render the branching narrative structure: the primary spine of stages, the
/// player-choice fork points with their options, and the endings.
///
/// Structural (stage names, topology) — it needs no filled prose, so it renders a
/// world straight out of `gen` without `--fill`. A fork point is a stage where the
/// spine can branch; its options list where each edge leads.
///
/// ```
/// use monomyth_core::doc_support::single_room_world;
/// use monomyth_text::render_structure;
///
/// let world = single_room_world();
/// // A one-node structure has a spine of one stage and no choices.
/// assert!(render_structure(&world).starts_with("Narrative structure"));
/// ```
#[must_use]
pub fn render_structure(world: &World) -> String {
    let structure = &world.story.structure;
    let mut out = String::new();
    let plot = world
        .story
        .plot
        .as_ref()
        .map(monomyth_core::ScoredOne::primary);
    let _ = writeln!(out, "Narrative structure (plot: {plot:?})");

    let _ = writeln!(out, "\nSpine:");
    for id in structure.spine() {
        let marker = if structure.is_fork(id) {
            "  [choice point]"
        } else {
            ""
        };
        let _ = writeln!(out, "  -> {}{marker}", stage_label(structure, id));
    }

    let mut forks = structure
        .nodes
        .iter()
        .filter(|&(id, _)| structure.is_fork(id))
        .peekable();
    if forks.peek().is_some() {
        let _ = writeln!(out, "\nChoices:");
        for (_, node) in forks {
            let _ = writeln!(out, "  at {:?}:", node.stage.primary());
            for edge in &node.out {
                let verb = match edge.kind {
                    EdgeKind::Choice => "enter",
                    EdgeKind::Sequence => "continue to",
                    EdgeKind::Fork => "branch to",
                };
                let _ = writeln!(out, "    - {verb} {}", stage_label(structure, edge.target));
            }
        }
    }

    let endings: Vec<String> = structure
        .endings()
        .map(|id| stage_label(structure, id))
        .collect();
    let _ = write!(out, "\nEndings: {}", endings.join(", "));
    out
}

/// Render the choices open at the narrative cursor as a numbered list.
///
/// Each line is `  <n>) <verb> <stage>`, where the number is what a player types to
/// take that branch and the verb reflects the edge kind (a linear continuation
/// versus a fork). The numbering matches
/// [`NarrativeStructure::available_choices`](monomyth_core::NarrativeStructure::available_choices),
/// so a chosen number resolves to the same edge. When the cursor is at an ending
/// (no open branch), a single closing line is returned instead.
///
/// ```
/// use monomyth_core::doc_support::single_room_world;
/// use monomyth_text::render_choices;
///
/// // The one-node fixture sits at its sole ending: no branches remain.
/// let world = single_room_world();
/// assert_eq!(render_choices(&world), "The story has reached an ending.");
/// ```
#[must_use]
pub fn render_choices(world: &World) -> String {
    let structure = &world.story.structure;
    let choices = structure.available_choices(world.state.cursor, &world.state.flags);
    if choices.is_empty() {
        return String::from("The story has reached an ending.");
    }
    let mut out = String::from("What next:");
    for (index, edge) in choices.iter().enumerate() {
        let verb = match edge.kind {
            EdgeKind::Sequence => "continue to",
            EdgeKind::Choice => "choose",
            EdgeKind::Fork => "branch to",
        };
        let _ = write!(
            out,
            "\n  {}) {verb} {}",
            index + 1,
            stage_label(structure, edge.target),
        );
    }
    out
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
fn slot_text(content: &Content) -> Cow<'_, str> {
    if let Some(value) = content.value() {
        return Cow::Borrowed(value.as_str());
    }
    let hint = &content.prompt().hint;
    if hint.is_empty() {
        Cow::Borrowed(UNNAMED_PLACEHOLDER)
    } else {
        Cow::Owned(format!("[{hint}]"))
    }
}

/// The comma-separated exit directions of a location, or `none` when it has none.
fn exits_line(location: &Location) -> String {
    if location.exits.is_empty() {
        return String::from("none");
    }
    location
        .exits
        .keys()
        .map(|&direction| direction_name(direction))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Join a sequence of already-resolved names with `", "`.
fn comma_separated<'a>(names: impl Iterator<Item = Cow<'a, str>>) -> String {
    names.map(Cow::into_owned).collect::<Vec<_>>().join(", ")
}

/// The lowercase name of a compass or vertical direction.
const fn direction_name(direction: Direction) -> &'static str {
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
fn location_name(world: &World, id: LocationId) -> Cow<'_, str> {
    world
        .locations
        .get(id)
        .map_or(Cow::Borrowed(MISSING_LOCATION), |location| {
            slot_text(&location.name)
        })
}

/// The name of an item by id, or a placeholder if the id does not resolve.
fn item_name(world: &World, id: ItemId) -> Cow<'_, str> {
    world
        .items
        .get(id)
        .map_or(Cow::Borrowed(MISSING_ITEM), |item| slot_text(&item.name))
}

/// The name of an entity by id, or a placeholder if the id does not resolve.
fn entity_name(world: &World, id: EntityId) -> Cow<'_, str> {
    world
        .entities
        .get(id)
        .map_or(Cow::Borrowed(MISSING_ENTITY), |entity| {
            slot_text(&entity.name)
        })
}

#[cfg(test)]
mod tests {
    use monomyth_core::doc_support::single_room_world;
    use monomyth_core::{
        Content, ContentKind, ContentPrompt, Direction, Entity, EntityKind, Event, Item, Location,
        LocationId, Provenance, World,
    };

    use super::{
        render_choices, render_event, render_events, render_intro, render_location,
        render_structure,
    };

    /// A filled content slot carrying `value`, for building test worlds.
    fn filled(kind: ContentKind, value: &str) -> Content {
        Content::filled(
            value.to_string(),
            ContentPrompt::new(kind, "hint"),
            Provenance::procedural("test"),
        )
    }

    /// A one-room world with every slot filled plus a north exit, an item, and an
    /// entity, so the full render path is exercised.
    fn filled_world() -> World {
        let mut world = single_room_world();
        let room = world.player.location;

        let hall = world.locations.insert(Location {
            name: filled(ContentKind::Name, "Great Hall"),
            description: filled(ContentKind::Description, "A vast echoing hall."),
            exits: std::collections::BTreeMap::new(),
            entities: std::collections::BTreeSet::new(),
            items: std::collections::BTreeSet::new(),
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

    /// The single non-player location id in a [`filled_world`].
    fn other_location(world: &World) -> LocationId {
        world
            .locations
            .keys()
            .find(|&id| id != world.player.location)
            .expect("a second location exists")
    }

    #[test]
    fn should_render_placeholder_for_unfilled_location() {
        let world = single_room_world();
        assert_eq!(
            render_location(&world),
            "[the antechamber]\n[a bare stone room]\n\nExits: none",
        );
    }

    #[test]
    fn should_render_filled_location_with_exits_items_and_entities() {
        let world = filled_world();
        assert_eq!(
            render_location(&world),
            "Antechamber\nA bare stone room.\n\nExits: north\nItems here: iron sword\nYou see: sphinx",
        );
    }

    #[test]
    fn should_use_unnamed_token_when_hint_is_blank() {
        let mut world = single_room_world();
        let room = world.player.location;
        let ghost = world.items.insert(Item {
            name: Content::empty(ContentPrompt::new(ContentKind::Name, "")),
            description: Content::empty(ContentPrompt::new(ContentKind::Description, "")),
            portable: true,
        });
        world
            .locations
            .get_mut(room)
            .expect("room exists")
            .items
            .insert(ghost);
        assert!(render_location(&world).contains("Items here: [unnamed]"));
    }

    #[test]
    fn should_render_moved_event_with_destination_name() {
        let world = filled_world();
        let event = Event::Moved {
            from: world.player.location,
            to: other_location(&world),
        };
        assert_eq!(
            render_event(&event, &world),
            "You travel to the Great Hall."
        );
    }

    #[test]
    fn should_render_took_event_with_item_name() {
        let world = filled_world();
        let item = world.items.keys().next().expect("an item exists");
        assert_eq!(
            render_event(&Event::Took(item), &world),
            "You take the iron sword."
        );
    }

    #[test]
    fn should_render_dropped_event_with_item_name() {
        let world = filled_world();
        let item = world.items.keys().next().expect("an item exists");
        assert_eq!(
            render_event(&Event::Dropped(item), &world),
            "You drop the iron sword."
        );
    }

    #[test]
    fn should_render_examined_event_verbatim() {
        let world = single_room_world();
        let event = Event::Examined {
            text: "A bare stone room.".to_string(),
        };
        assert_eq!(render_event(&event, &world), "A bare stone room.");
    }

    #[test]
    fn should_render_waited_event() {
        let world = single_room_world();
        assert_eq!(render_event(&Event::Waited, &world), "You wait.");
    }

    #[test]
    fn should_render_placeholder_for_unresolved_item_id() {
        let mut world = filled_world();
        let item = world.items.keys().next().expect("an item exists");
        world.items.remove(item);
        assert_eq!(
            render_event(&Event::Took(item), &world),
            "You take the [unknown item]."
        );
    }

    #[test]
    fn should_join_multiple_events_with_newlines() {
        let world = single_room_world();
        let events = [Event::Waited, Event::Waited];
        assert_eq!(render_events(&events, &world), "You wait.\nYou wait.");
    }

    #[test]
    fn should_render_intro_from_title_and_seed() {
        let world = single_room_world();
        assert_eq!(render_intro(&world), "[an untitled world]\n\nSeed: 0");
    }

    #[test]
    fn should_render_structure_with_a_fork() {
        use std::collections::BTreeSet;

        use monomyth_core::{EdgeKind, NarrativeEdge, NarrativeNode, NarrativeStructure, NodeKind};
        use monomyth_frameworks::{BookerPlot, MonomythStage};

        fn empty(kind: ContentKind) -> Content {
            Content::empty(ContentPrompt::new(kind, ""))
        }

        let mut structure = NarrativeStructure::default();
        let end = structure.nodes.insert(NarrativeNode::new(
            "FreedomToLive",
            NodeKind::Ending,
            MonomythStage::FreedomToLive,
            empty(ContentKind::Synopsis),
        ));
        let mid = structure.nodes.insert(NarrativeNode::new(
            "RefusalOfTheCall",
            NodeKind::Beat,
            MonomythStage::RefusalOfTheCall,
            empty(ContentKind::Synopsis),
        ));
        let mut origin = NarrativeNode::new(
            "CallToAdventure",
            NodeKind::Origin,
            MonomythStage::CallToAdventure,
            empty(ContentKind::Synopsis),
        );
        origin.out.push(NarrativeEdge::new(
            end,
            EdgeKind::Sequence,
            empty(ContentKind::Choice),
        ));
        origin.out.push(NarrativeEdge::new(
            mid,
            EdgeKind::Choice,
            empty(ContentKind::Choice),
        ));
        let root = structure.nodes.insert(origin);
        structure.nodes[mid].out.push(NarrativeEdge::new(
            end,
            EdgeKind::Sequence,
            empty(ContentKind::Choice),
        ));
        structure.root = root;
        structure.endings = BTreeSet::from([end]);

        let mut world = single_room_world();
        world.story.structure = structure;
        world.story.plot = Some(BookerPlot::RagsToRiches.into());

        let expected = "Narrative structure (plot: Some(RagsToRiches))\n\
             \nSpine:\n\
             \u{20}\u{20}-> CallToAdventure  [choice point]\n\
             \u{20}\u{20}-> FreedomToLive\n\
             \nChoices:\n\
             \u{20}\u{20}at CallToAdventure:\n\
             \u{20}\u{20}\u{20}\u{20}- continue to FreedomToLive\n\
             \u{20}\u{20}\u{20}\u{20}- enter RefusalOfTheCall\n\
             \nEndings: FreedomToLive";
        assert_eq!(render_structure(&world), expected);
    }

    #[test]
    fn should_render_numbered_choices_at_a_fork() {
        use std::collections::BTreeSet;

        use monomyth_core::{EdgeKind, NarrativeEdge, NarrativeNode, NarrativeStructure, NodeKind};
        use monomyth_frameworks::MonomythStage;

        fn empty(kind: ContentKind) -> Content {
            Content::empty(ContentPrompt::new(kind, ""))
        }

        let mut structure = NarrativeStructure::default();
        let end = structure.nodes.insert(NarrativeNode::new(
            "FreedomToLive",
            NodeKind::Ending,
            MonomythStage::FreedomToLive,
            empty(ContentKind::Synopsis),
        ));
        let mid = structure.nodes.insert(NarrativeNode::new(
            "RefusalOfTheCall",
            NodeKind::Beat,
            MonomythStage::RefusalOfTheCall,
            empty(ContentKind::Synopsis),
        ));
        let mut origin = NarrativeNode::new(
            "CallToAdventure",
            NodeKind::Origin,
            MonomythStage::CallToAdventure,
            empty(ContentKind::Synopsis),
        );
        origin.out.push(NarrativeEdge::new(
            end,
            EdgeKind::Sequence,
            empty(ContentKind::Choice),
        ));
        origin.out.push(NarrativeEdge::new(
            mid,
            EdgeKind::Choice,
            empty(ContentKind::Choice),
        ));
        let root = structure.nodes.insert(origin);
        structure.nodes[mid].out.push(NarrativeEdge::new(
            end,
            EdgeKind::Sequence,
            empty(ContentKind::Choice),
        ));
        structure.root = root;
        structure.endings = BTreeSet::from([end]);

        let mut world = single_room_world();
        world.story.structure = structure;
        world.state.cursor = root;

        let expected = "What next:\n\
             \u{20}\u{20}1) continue to FreedomToLive\n\
             \u{20}\u{20}2) choose RefusalOfTheCall";
        assert_eq!(render_choices(&world), expected);

        let advanced = Event::Advanced {
            from: root,
            to: mid,
        };
        assert_eq!(
            render_event(&advanced, &world),
            "The story moves on to RefusalOfTheCall."
        );

        world.state.cursor = end;
        assert_eq!(render_choices(&world), "The story has reached an ending.");
    }
}
