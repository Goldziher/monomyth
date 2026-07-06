//! Character-model taxonomies: the three orthogonal facets of a dramatic character.
//!
//! - [`ProppRole`] — Propp's seven spheres of action.
//! - [`GreimasActant`] — Greimas's six actants on three axes.
//! - [`Archetype`] — the eight Jungian/Vogler story archetypes.

use std::sync::LazyLock;

use crate::info::{ArchetypeInfo, GreimasActantInfo, ProppRoleInfo, parse_items};
use crate::macros::framework_enum;

static PROPP_ROLES: LazyLock<Vec<ProppRoleInfo>> = LazyLock::new(|| {
    parse_items(include_str!(
        "../../../artifacts/frameworks/propp_roles.json"
    ))
});

static GREIMAS_ACTANTS: LazyLock<Vec<GreimasActantInfo>> = LazyLock::new(|| {
    parse_items(include_str!(
        "../../../artifacts/frameworks/greimas_actants.json"
    ))
});

static ARCHETYPES: LazyLock<Vec<ArchetypeInfo>> = LazyLock::new(|| {
    parse_items(include_str!(
        "../../../artifacts/frameworks/archetypes.json"
    ))
});

framework_enum! {
    /// Propp's seven dramatis-personae roles / spheres of action (character tier).
    ProppRole : ProppRoleInfo = PROPP_ROLES;
    Villain = 1,
    Donor = 2,
    Helper = 3,
    Princess = 4,
    Dispatcher = 5,
    Hero = 6,
    FalseHero = 7,
}

framework_enum! {
    /// Greimas's six actants — the clean relational layer (character tier).
    GreimasActant : GreimasActantInfo = GREIMAS_ACTANTS;
    Subject = 1,
    Object = 2,
    Sender = 3,
    Receiver = 4,
    Helper = 5,
    Opponent = 6,
}

framework_enum! {
    /// The eight story archetypes, a function-flexible character facet (character tier).
    Archetype : ArchetypeInfo = ARCHETYPES;
    Hero = 1,
    Mentor = 2,
    ThresholdGuardian = 3,
    Herald = 4,
    Shapeshifter = 5,
    Shadow = 6,
    Ally = 7,
    Trickster = 8,
}
