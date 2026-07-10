//! [`EntityContentPass`]: fill every entity's name and description slots.

use async_trait::async_trait;
use monomyth_core::{EntityId, Provenance, World};

use crate::content::{
    ContentContext, ContentPass, GROUNDING_TOP_K, NamedProse, build_prompt, ground, grounding_note,
};
use crate::error::GenError;

/// The schema name passed to the LLM for each entity generation call.
const ENTITY_SCHEMA: &str = "entity_prose";
/// The writing task handed to the model for an entity.
const ENTITY_INSTRUCTION: &str = "Write the proper name and a vivid one-paragraph description for a character or being \
     in a mythic world, true to its dramatic role.";

/// Fills the `name` and `description` slots of every [`World::entities`] entry,
/// grounding each generation on the entity's role hint.
#[derive(Debug, Default, Clone, Copy)]
pub struct EntityContentPass;

impl EntityContentPass {
    /// Construct the entity content pass.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ContentPass for EntityContentPass {
    fn name(&self) -> &'static str {
        "EntityContentPass"
    }

    async fn apply(&self, world: &mut World, context: &ContentContext<'_>) -> Result<(), GenError> {
        let ids: Vec<EntityId> = world.entities.keys().collect();
        for id in ids {
            let entity = &world.entities[id];
            let name_hint = entity.name.prompt().hint.clone();
            let description_hint = entity.description.prompt().hint.clone();

            let passages = ground(context, &name_hint, GROUNDING_TOP_K).await?;
            let slot_hint = format!("name: {name_hint}; description: {description_hint}");
            let prompt = build_prompt(ENTITY_INSTRUCTION, &slot_hint, &passages);
            let generated = context
                .llm
                .generate::<NamedProse>(&prompt, ENTITY_SCHEMA)
                .await?;

            let note = grounding_note(self.name(), &passages);
            let entity = &mut world.entities[id];
            entity.name.fill(
                generated.value.name,
                Provenance::llm(context.model, note.clone()),
            );
            entity.description.fill(
                generated.value.description,
                Provenance::llm(context.model, note),
            );
        }
        Ok(())
    }
}
