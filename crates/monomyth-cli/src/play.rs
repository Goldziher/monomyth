//! The interactive play loop and its pure command parser.
//!
//! [`parse_command`] is a pure function of a line and the current [`World`]; the
//! loop in [`run_play`] only handles IO and dispatch. Keeping the parser pure lets
//! the whole grammar be unit-tested without stdin or a running engine.

use std::io::{self, Write};

use anyhow::{Context, Result};
use monomyth_core::{Action, Content, Direction, EntityId, ExamineTarget, ItemId, World, apply};
use monomyth_text::{render_events, render_intro, render_location};

/// A parsed line: either an engine action to apply, or a request to quit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PlayCommand {
    /// An action to feed to [`monomyth_core::apply`].
    Act(Action),
    /// End the session.
    Quit,
}

/// Parse one input line against `world` into a [`PlayCommand`].
///
/// The grammar is case-insensitive and forgiving:
/// - `n`/`s`/`e`/`w`/`up`/`down`, or `move <dir>` / `go <dir>` → [`Action::Move`].
/// - `look`, or `examine`/`x` with no argument → examine the current location.
/// - `examine <name>` / `x <name>` → examine a named item or entity in the room.
/// - `take <name>` / `get <name>` → take a named item from the room.
/// - `drop <name>` → drop a named item from the inventory.
/// - `wait` / `z` → [`Action::Wait`].
/// - `quit` / `exit` → [`PlayCommand::Quit`].
///
/// Name resolution matches filled slot values case-insensitively; unfilled slots
/// simply do not match. Unknown verbs and unresolved names return an `Err(String)`
/// message the caller prints before continuing.
///
/// # Errors
///
/// Returns a human-readable message when the verb is unknown, a direction is
/// invalid, or a named target cannot be resolved.
pub(crate) fn parse_command(line: &str, world: &World) -> Result<PlayCommand, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Err("Say something, or type `quit` to leave.".to_owned());
    }
    let (verb, argument) = match trimmed.split_once(char::is_whitespace) {
        Some((verb, argument)) => (verb, argument.trim()),
        None => (trimmed, ""),
    };

    match verb.to_lowercase().as_str() {
        "quit" | "exit" => Ok(PlayCommand::Quit),
        "wait" | "z" => Ok(PlayCommand::Act(Action::Wait)),
        "look" => Ok(PlayCommand::Act(Action::Examine(ExamineTarget::Location))),
        "examine" | "x" if argument.is_empty() => {
            Ok(PlayCommand::Act(Action::Examine(ExamineTarget::Location)))
        }
        "examine" | "x" => resolve_examine(argument, world),
        "move" | "go" => {
            let direction = parse_direction(argument)
                .ok_or_else(|| format!("You cannot go that way: {argument:?}."))?;
            Ok(PlayCommand::Act(Action::Move(direction)))
        }
        "take" | "get" => resolve_take(argument, world),
        "drop" => resolve_drop(argument, world),
        other => bare_direction(other)
            .filter(|_| argument.is_empty())
            .map(|direction| PlayCommand::Act(Action::Move(direction)))
            .ok_or_else(|| format!("I don't understand: {trimmed:?}.")),
    }
}

/// Run the interactive loop over stdin until `quit`/`exit` or end of input.
///
/// A parse error or [`monomyth_core::ActionError`] prints a message and continues;
/// only an IO failure aborts.
///
/// # Errors
///
/// Fails only if reading a line from stdin fails.
pub(crate) fn run_play(mut world: World) -> Result<()> {
    println!("{}\n", render_intro(&world));
    println!("{}", render_location(&world));

    let stdin = io::stdin();
    let mut line = String::new();
    loop {
        print!("\n> ");
        io::stdout().flush().ok();

        line.clear();
        let read = stdin.read_line(&mut line).context("reading player input")?;
        if read == 0 {
            break; // end of input
        }

        match parse_command(&line, &world) {
            Ok(PlayCommand::Quit) => break,
            Ok(PlayCommand::Act(action)) => match apply(&mut world, action) {
                Ok(events) => {
                    let rendered = render_events(&events, &world);
                    if !rendered.is_empty() {
                        println!("{rendered}");
                    }
                    println!("\n{}", render_location(&world));
                }
                Err(error) => println!("{error}"),
            },
            Err(message) => println!("{message}"),
        }
    }
    println!("Farewell.");
    Ok(())
}

/// Parse a direction from `move`/`go` arguments, accepting short and long forms.
fn parse_direction(token: &str) -> Option<Direction> {
    match token.to_lowercase().as_str() {
        "n" | "north" => Some(Direction::North),
        "s" | "south" => Some(Direction::South),
        "e" | "east" => Some(Direction::East),
        "w" | "west" => Some(Direction::West),
        "u" | "up" => Some(Direction::Up),
        "d" | "down" => Some(Direction::Down),
        _ => None,
    }
}

/// Parse a bare direction verb (the long forms only, to avoid clashing with
/// verbs like `d`rop or `e`xamine when a word is typed alone).
fn bare_direction(token: &str) -> Option<Direction> {
    match token {
        "n" | "north" => Some(Direction::North),
        "s" | "south" => Some(Direction::South),
        "e" | "east" => Some(Direction::East),
        "w" | "west" => Some(Direction::West),
        "up" => Some(Direction::Up),
        "down" => Some(Direction::Down),
        _ => None,
    }
}

/// Resolve `examine <name>` to an item or entity in the current room.
fn resolve_examine(name: &str, world: &World) -> Result<PlayCommand, String> {
    if let Some(item) = find_room_item(world, name) {
        return Ok(PlayCommand::Act(Action::Examine(ExamineTarget::Item(item))));
    }
    if let Some(entity) = find_room_entity(world, name) {
        return Ok(PlayCommand::Act(Action::Examine(ExamineTarget::Entity(
            entity,
        ))));
    }
    Err(format!("You see no {name} here."))
}

/// Resolve `take <name>` to an item in the current room.
fn resolve_take(name: &str, world: &World) -> Result<PlayCommand, String> {
    find_room_item(world, name)
        .map(|item| PlayCommand::Act(Action::Take(item)))
        .ok_or_else(|| format!("There is no {name} to take here."))
}

/// Resolve `drop <name>` to an item in the player's inventory.
fn resolve_drop(name: &str, world: &World) -> Result<PlayCommand, String> {
    world
        .player
        .inventory
        .iter()
        .copied()
        .find(|&id| {
            world
                .items
                .get(id)
                .is_some_and(|item| name_matches(&item.name, name))
        })
        .map(|item| PlayCommand::Act(Action::Drop(item)))
        .ok_or_else(|| format!("You are not carrying a {name}."))
}

/// Find an item in the current room whose filled name matches `name`.
fn find_room_item(world: &World, name: &str) -> Option<ItemId> {
    let location = world.locations.get(world.player.location)?;
    location.items.iter().copied().find(|&id| {
        world
            .items
            .get(id)
            .is_some_and(|item| name_matches(&item.name, name))
    })
}

/// Find an entity in the current room whose filled name matches `name`.
fn find_room_entity(world: &World, name: &str) -> Option<EntityId> {
    let location = world.locations.get(world.player.location)?;
    location.entities.iter().copied().find(|&id| {
        world
            .entities
            .get(id)
            .is_some_and(|entity| name_matches(&entity.name, name))
    })
}

/// Whether a content slot is filled with a value equal to `query`, ignoring case.
fn name_matches(content: &Content, query: &str) -> bool {
    content
        .value()
        .is_some_and(|value| value.eq_ignore_ascii_case(query))
}

#[cfg(test)]
mod tests {
    use monomyth_core::doc_support::single_room_world;
    use monomyth_core::{
        Action, Content, ContentKind, ContentPrompt, Direction, Entity, EntityId, EntityKind,
        ExamineTarget, Item, ItemId, Provenance, World,
    };

    use super::{PlayCommand, parse_command};

    fn filled(kind: ContentKind, value: &str) -> Content {
        Content::filled(
            value.to_owned(),
            ContentPrompt::new(kind, "hint"),
            Provenance::procedural("test"),
        )
    }

    /// A one-room world holding a room item, a room entity, and an inventory item,
    /// so every name-resolution branch can be exercised.
    fn test_world() -> World {
        let mut world = single_room_world();
        let room = world.player.location;

        let sword = world.items.insert(Item {
            name: filled(ContentKind::Name, "iron sword"),
            description: filled(ContentKind::Description, "A pitted blade."),
            portable: true,
        });
        let lamp = world.items.insert(Item {
            name: filled(ContentKind::Name, "brass lamp"),
            description: filled(ContentKind::Description, "It glows faintly."),
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
        location.items.insert(sword);
        location.entities.insert(sphinx);
        world.player.inventory.insert(lamp);
        world
    }

    fn item_named(world: &World, name: &str) -> ItemId {
        world
            .items
            .keys()
            .find(|&id| world.items[id].name.value().expect("filled") == name)
            .expect("named item exists")
    }

    fn entity_named(world: &World, name: &str) -> EntityId {
        world
            .entities
            .keys()
            .find(|&id| world.entities[id].name.value().expect("filled") == name)
            .expect("named entity exists")
    }

    #[test]
    fn should_parse_bare_direction() {
        let world = test_world();
        assert_eq!(
            parse_command("n", &world),
            Ok(PlayCommand::Act(Action::Move(Direction::North)))
        );
    }

    #[test]
    fn should_parse_go_direction() {
        let world = test_world();
        assert_eq!(
            parse_command("go DOWN", &world),
            Ok(PlayCommand::Act(Action::Move(Direction::Down)))
        );
    }

    #[test]
    fn should_error_on_unknown_direction() {
        let world = test_world();
        assert_eq!(
            parse_command("move nowhere", &world),
            Err("You cannot go that way: \"nowhere\".".to_owned())
        );
    }

    #[test]
    fn should_parse_look_as_examine_location() {
        let world = test_world();
        assert_eq!(
            parse_command("look", &world),
            Ok(PlayCommand::Act(Action::Examine(ExamineTarget::Location)))
        );
    }

    #[test]
    fn should_parse_bare_examine_as_location() {
        let world = test_world();
        assert_eq!(
            parse_command("examine", &world),
            Ok(PlayCommand::Act(Action::Examine(ExamineTarget::Location)))
        );
    }

    #[test]
    fn should_resolve_examine_item_by_name() {
        let world = test_world();
        let sword = item_named(&world, "iron sword");
        assert_eq!(
            parse_command("x Iron Sword", &world),
            Ok(PlayCommand::Act(Action::Examine(ExamineTarget::Item(
                sword
            ))))
        );
    }

    #[test]
    fn should_resolve_examine_entity_by_name() {
        let world = test_world();
        let sphinx = entity_named(&world, "sphinx");
        assert_eq!(
            parse_command("examine sphinx", &world),
            Ok(PlayCommand::Act(Action::Examine(ExamineTarget::Entity(
                sphinx
            ))))
        );
    }

    #[test]
    fn should_resolve_take_room_item() {
        let world = test_world();
        let sword = item_named(&world, "iron sword");
        assert_eq!(
            parse_command("get iron sword", &world),
            Ok(PlayCommand::Act(Action::Take(sword)))
        );
    }

    #[test]
    fn should_resolve_drop_inventory_item() {
        let world = test_world();
        let lamp = item_named(&world, "brass lamp");
        assert_eq!(
            parse_command("drop brass lamp", &world),
            Ok(PlayCommand::Act(Action::Drop(lamp)))
        );
    }

    #[test]
    fn should_not_take_an_inventory_only_item() {
        let world = test_world();
        assert_eq!(
            parse_command("take brass lamp", &world),
            Err("There is no brass lamp to take here.".to_owned())
        );
    }

    #[test]
    fn should_parse_wait_aliases() {
        let world = test_world();
        assert_eq!(
            parse_command("wait", &world),
            Ok(PlayCommand::Act(Action::Wait))
        );
        assert_eq!(
            parse_command("z", &world),
            Ok(PlayCommand::Act(Action::Wait))
        );
    }

    #[test]
    fn should_parse_quit_and_exit() {
        let world = test_world();
        assert_eq!(parse_command("quit", &world), Ok(PlayCommand::Quit));
        assert_eq!(parse_command("EXIT", &world), Ok(PlayCommand::Quit));
    }

    #[test]
    fn should_error_on_unknown_command() {
        let world = test_world();
        assert_eq!(
            parse_command("flibbertigibbet", &world),
            Err("I don't understand: \"flibbertigibbet\".".to_owned())
        );
    }

    #[test]
    fn should_error_on_unresolved_examine_target() {
        let world = test_world();
        assert_eq!(
            parse_command("examine unicorn", &world),
            Err("You see no unicorn here.".to_owned())
        );
    }

    #[test]
    fn should_error_on_empty_input() {
        let world = test_world();
        assert_eq!(
            parse_command("   ", &world),
            Err("Say something, or type `quit` to leave.".to_owned())
        );
    }
}
