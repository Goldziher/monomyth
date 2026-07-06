//! Hidden fixtures used by the crate's doctests.
//!
//! These are `#[doc(hidden)]` so they do not appear in the public documentation,
//! but they are compiled into the crate so runnable examples can build a real
//! [`World`] without duplicating the whole struct literal in prose.

use std::collections::{BTreeMap, BTreeSet};

use monomyth_frameworks::MonomythStage;
use slotmap::SlotMap;

use crate::content::{Content, ContentKind, ContentPrompt};
use crate::entity::Player;
use crate::narrative::{NarrativeNode, NarrativeStructure, NodeKind};
use crate::rng::RngState;
use crate::story::Story;
use crate::world::{Location, SCHEMA_VERSION, World, WorldMeta, WorldState};

/// A minimal single-node narrative structure whose sole node is both the root and
/// the only ending.
///
/// The node has out-degree 0, so it is tagged [`NodeKind::Ending`] (terminality is
/// its defining topological property); a single node is trivially a valid,
/// reconverging DAG.
#[doc(hidden)]
#[must_use]
pub fn single_node_structure() -> NarrativeStructure {
    let mut nodes = SlotMap::with_key();
    let root = nodes.insert(NarrativeNode::new(
        "FreedomToLive",
        NodeKind::Ending,
        MonomythStage::FreedomToLive,
        Content::empty(ContentPrompt::new(ContentKind::Synopsis, "the single beat")),
    ));
    NarrativeStructure {
        nodes,
        root,
        endings: BTreeSet::from([root]),
    }
}

/// A minimal one-room world with no exits, for examples.
#[doc(hidden)]
#[must_use]
pub fn single_room_world() -> World {
    let mut locations = SlotMap::with_key();
    let room = locations.insert(Location {
        name: Content::empty(ContentPrompt::new(ContentKind::Name, "the antechamber")),
        description: Content::empty(ContentPrompt::new(
            ContentKind::Description,
            "a bare stone room",
        )),
        exits: BTreeMap::new(),
        entities: BTreeSet::new(),
        items: BTreeSet::new(),
    });
    let structure = single_node_structure();
    let cursor = structure.root();
    World {
        meta: WorldMeta {
            seed: 0,
            schema_version: SCHEMA_VERSION,
            title: Content::empty(ContentPrompt::new(ContentKind::Title, "an untitled world")),
        },
        locations,
        entities: SlotMap::with_key(),
        items: SlotMap::with_key(),
        player: Player::new(room),
        story: Story {
            structure,
            plot: None,
            quests: SlotMap::with_key(),
        },
        state: WorldState {
            cursor,
            ..WorldState::default()
        },
        rng: RngState::new(0),
    }
}
