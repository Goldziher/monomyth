//! [`ItemContentPass`]: fill every item's name and description slots.

use async_trait::async_trait;
use monomyth_core::{ItemId, Provenance, World};

use crate::content::{
    ContentContext, ContentPass, GROUNDING_TOP_K, NamedProse, build_prompt, ground, grounding_note,
};
use crate::error::GenError;

/// The schema name passed to the LLM for each item generation call.
const ITEM_SCHEMA: &str = "item_prose";
/// The writing task handed to the model for an item.
const ITEM_INSTRUCTION: &str =
    "Write the proper name and a vivid one-paragraph description for an object in a mythic world.";

/// Fills the `name` and `description` slots of every [`World::items`] entry.
#[derive(Debug, Default, Clone, Copy)]
pub struct ItemContentPass;

impl ItemContentPass {
    /// Construct the item content pass.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ContentPass for ItemContentPass {
    fn name(&self) -> &'static str {
        "ItemContentPass"
    }

    async fn apply(&self, world: &mut World, context: &ContentContext<'_>) -> Result<(), GenError> {
        let ids: Vec<ItemId> = world.items.keys().collect();
        for id in ids {
            let item = &world.items[id];
            let name_hint = item.name.prompt().hint.clone();
            let description_hint = item.description.prompt().hint.clone();

            let passages = ground(context, &name_hint, GROUNDING_TOP_K).await?;
            let slot_hint = format!("name: {name_hint}; description: {description_hint}");
            let prompt = build_prompt(ITEM_INSTRUCTION, &slot_hint, &passages);
            let generated = context
                .llm
                .generate::<NamedProse>(&prompt, ITEM_SCHEMA)
                .await?;

            let note = grounding_note(self.name(), &passages);
            let item = &mut world.items[id];
            item.name.fill(
                generated.value.name,
                Provenance::llm(context.model, note.clone()),
            );
            item.description.fill(
                generated.value.description,
                Provenance::llm(context.model, note),
            );
        }
        Ok(())
    }
}
