//! [`CastPass`]: populate the world with role-driven entities.
//!
//! Every world has at least a [`Hero`](monomyth_frameworks::ProppRole::Hero) and a
//! [`Villain`](monomyth_frameworks::ProppRole::Villain); a seed-chosen number of
//! supporting roles round out the cast. Each entity's archetype is drawn from the
//! Propp-role crosswalk, and it is placed into a room with both sides of the
//! entity/location relation kept in sync. Name and description slots stay empty,
//! hinted with the Propp role.

use monomyth_core::{
    Content, ContentKind, ContentPrompt, Entity, EntityKind, ScoredOne, Weight, World,
};
use monomyth_frameworks::{ProppRole, role_archetypes_weighted};
use rand_chacha::ChaCha8Rng;

use crate::error::GenError;
use crate::pass::{ProceduralPass, draw_range_inclusive, draw_weighted_index};

/// The roles every cast contains, guaranteeing a protagonist and an antagonist.
const BASE_ROLES: [ProppRole; 2] = [ProppRole::Hero, ProppRole::Villain];

/// The pool of supporting roles, drawn from in order after the base roles.
const EXTRA_ROLE_POOL: [ProppRole; 5] = [
    ProppRole::Dispatcher,
    ProppRole::Donor,
    ProppRole::Helper,
    ProppRole::Princess,
    ProppRole::FalseHero,
];

/// The most supporting roles added on top of the base cast.
const MAX_EXTRA_CAST: usize = 3;

/// Builds the cast of role-bearing entities and places them in rooms.
#[derive(Debug, Default, Clone, Copy)]
pub struct CastPass;

impl CastPass {
    /// Construct the cast pass.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl ProceduralPass for CastPass {
    fn name(&self) -> &'static str {
        "CastPass"
    }

    fn apply(&self, world: &mut World, rng: &mut ChaCha8Rng) -> Result<(), GenError> {
        let rooms: Vec<_> = world.locations.keys().collect();
        if rooms.is_empty() {
            return Err(GenError::NoLocations { pass: self.name() });
        }

        let extra_count = draw_range_inclusive(rng, 0, MAX_EXTRA_CAST);
        let roles = BASE_ROLES
            .iter()
            .copied()
            .chain(EXTRA_ROLE_POOL.iter().copied().take(extra_count));

        for role in roles {
            let archetype = draw_archetype(rng, role);

            let name_hint = format!("name of the {role:?} character");
            let description_hint = format!("description of the {role:?} character");
            let room = rooms[draw_range_inclusive(rng, 0, rooms.len() - 1)];

            let entity = Entity {
                name: Content::empty(ContentPrompt::new(ContentKind::Name, name_hint)),
                description: Content::empty(ContentPrompt::new(
                    ContentKind::Description,
                    description_hint,
                )),
                kind: EntityKind::Npc,
                role: Some(ScoredOne::new(role)),
                archetype,
                location: Some(room),
            };

            let entity_id = world.entities.insert(entity);
            world.locations[room].entities.insert(entity_id);
        }

        Ok(())
    }
}

/// Draw a single weighted archetype from `role`'s crosswalk candidates
/// ([`role_archetypes_weighted`]), wrapped as a [`ScoredOne`] with no
/// alternatives, or `None` if the role has no assigned archetypes (or every
/// candidate weight is zero).
fn draw_archetype(
    rng: &mut ChaCha8Rng,
    role: ProppRole,
) -> Option<ScoredOne<monomyth_frameworks::Archetype>> {
    let candidates = role_archetypes_weighted(role);
    if candidates.is_empty() {
        return None;
    }
    let weights: Vec<Weight> = candidates
        .iter()
        .map(|&(_, permille)| Weight::new(permille))
        .collect();
    let index = draw_weighted_index(rng, &weights)?;
    Some(ScoredOne::new(candidates[index].0))
}
