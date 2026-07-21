//! Genre as a cross-cutting config dimension (ADR-0017).
//!
//! `monomyth-core`'s `World`/`Story` stay medium- *and* genre-agnostic (ADR-0014):
//! a detective story and a myth serialize to the same shapes. Genre instead
//! reaches generation the same way `NarrativeConfig` already does — as a config
//! value and a trait seam, never as a field on the contract. This crate holds the
//! two roles ADR-0017 keeps distinct:
//!
//! - [`GenreProfile`] — a config *value*, resolved through
//!   `monomyth-config`'s layered [`GenreSettings`](monomyth_config::GenreSettings),
//!   that biases content-fill targeting toward a genre's conventions. This is the
//!   role implemented now; see [`GenreProfile::content_config`].
//! - [`GenreClassifier`] — a trait seam that will infer a `GenreProfile` from
//!   input text once extraction (ADR-0018) exists. [`StubGenreClassifier`] is the
//!   only implementation for now: classification is explicitly deferred.
//!
//! `monomyth-core` must never depend on this crate; that invariant is enforced by
//! a `cargo tree` CI guard and by `monomyth-core/tests/boundaries.rs`.

#![forbid(unsafe_code)]

mod classifier;
mod profile;

pub use classifier::{GenreClassifier, GenreError, StubGenreClassifier};
pub use profile::{GenreKind, GenreProfile};
