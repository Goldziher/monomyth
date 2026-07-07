//! The concrete content passes that make up the default content pipeline.
//!
//! Each submodule fills one family of [`Content`](monomyth_core::Content) slots
//! with grounded, LLM-authored prose: [`TitleContentPass`] the world title,
//! [`LocationContentPass`] every location's name and description, and
//! [`EntityContentPass`] every entity's name and description, and
//! [`ItemContentPass`] every item's name and description. None of them alter
//! structure — they only turn empty slots into filled ones.
//!
//! Beat synopses and quest titles are a documented follow-up; leaving those slots
//! empty is fine, since [`Content`](monomyth_core::Content) supports partial fill.

mod entity;
mod item;
mod location;
mod title;

pub use entity::EntityContentPass;
pub use item::ItemContentPass;
pub use location::LocationContentPass;
pub use title::TitleContentPass;
