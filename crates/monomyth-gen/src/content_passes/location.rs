//! [`LocationContentPass`]: fill every location's name and description slots.

use async_trait::async_trait;
use monomyth_core::{LocationId, Provenance, World};

use crate::content::{
    ContentContext, ContentPass, NamedProse, build_prompt, ground, grounding_note,
};
use crate::error::GenError;

/// The schema name passed to the LLM for each location generation call.
const LOCATION_SCHEMA: &str = "location_prose";

/// Fills the `name` and `description` slots of every [`World::locations`] entry.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocationContentPass;

impl LocationContentPass {
    /// Construct the location content pass.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ContentPass for LocationContentPass {
    fn name(&self) -> &'static str {
        "LocationContentPass"
    }

    async fn apply(&self, world: &mut World, context: &ContentContext<'_>) -> Result<(), GenError> {
        let ids: Vec<LocationId> = world.locations.keys().collect();
        for id in ids {
            let location = &world.locations[id];
            let name_hint = location.name.prompt().hint.clone();
            let description_hint = location.description.prompt().hint.clone();

            let passages = ground(context, &name_hint, context.config.grounding_top_k).await?;
            let slot_hint = format!("name: {name_hint}; description: {description_hint}");
            let prompt = build_prompt(&context.config.location_instruction, &slot_hint, &passages);
            let generated = context
                .llm
                .generate::<NamedProse>(&prompt, LOCATION_SCHEMA)
                .await?;

            let note = grounding_note(self.name(), &passages);
            let location = &mut world.locations[id];
            location.name.fill(
                generated.value.name,
                Provenance::llm(context.model, note.clone()),
            );
            location.description.fill(
                generated.value.description,
                Provenance::llm(context.model, note),
            );
        }
        Ok(())
    }
}
