//! [`TitleContentPass`]: fill the world title slot with grounded prose.

use async_trait::async_trait;
use monomyth_core::{Provenance, World};

use crate::content::{
    ContentContext, ContentPass, TextProse, build_prompt, ground, grounding_note,
};
use crate::error::GenError;

/// The schema name passed to the LLM for the title generation call.
const TITLE_SCHEMA: &str = "world_title";

/// Fills [`World::meta`](monomyth_core::WorldMeta)'s `title` slot.
#[derive(Debug, Default, Clone, Copy)]
pub struct TitleContentPass;

impl TitleContentPass {
    /// Construct the title content pass.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

#[async_trait]
impl ContentPass for TitleContentPass {
    fn name(&self) -> &'static str {
        "TitleContentPass"
    }

    async fn apply(&self, world: &mut World, context: &ContentContext<'_>) -> Result<(), GenError> {
        let hint = world.meta.title.prompt().hint.clone();
        let passages = ground(context, &hint, context.config.grounding_top_k).await?;
        let prompt = build_prompt(&context.config.title_instruction, &hint, &passages);
        let generated = context
            .llm
            .generate::<TextProse>(&prompt, TITLE_SCHEMA)
            .await?;
        let provenance = Provenance::llm(context.model, grounding_note(self.name(), &passages));
        world.meta.title.fill(generated.value.text, provenance);
        Ok(())
    }
}
