//! [`GenreClassifier`]: the ADR-0018 seam that will infer a [`GenreProfile`] from
//! input text.
//!
//! Mirrors the style of `monomyth-contracts`'s `Classifier`/`Extractor` traits: a
//! `Send + Sync` async trait behind `dyn`, with a `thiserror` error enum. It lives
//! here rather than in `monomyth-contracts` because it returns a `GenreProfile`
//! owned by this crate — putting the trait in `monomyth-contracts` would force a
//! `monomyth-contracts` → `monomyth-genre` edge that ADR-0013's plane-boundary
//! discipline forbids.
//!
//! Classification itself is out of scope for ADR-0017 (deferred to ADR-0018, once
//! extraction gives it real signal to work with); [`StubGenreClassifier`] is the
//! only implementation for now.

use async_trait::async_trait;

use crate::profile::GenreProfile;

/// A genre-classification failure.
///
/// `#[non_exhaustive]` because the only implementation today is a stub that never
/// fails; a real classifier (ADR-0018) will add variants (e.g. a retrieval or
/// model failure) without that being a breaking change for existing callers.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GenreError {
    /// Classification could not produce a profile from the given text.
    #[error("genre classification failed: {0}")]
    Classification(String),
}

/// Infer a [`GenreProfile`] from a fragment of narrative text.
///
/// The targeting role ([`GenreProfile`] as a resolved config value) is
/// implemented now; this classification role is a stub until extraction
/// (ADR-0018) exists to make it meaningful. The trait seam is stable today so
/// ADR-0018 can consume it from its first slice rather than retrofitting one.
#[async_trait]
pub trait GenreClassifier: Send + Sync {
    /// Classify `text`, returning the [`GenreProfile`] it most resembles.
    ///
    /// # Errors
    ///
    /// Returns [`GenreError`] if classification cannot produce a profile.
    async fn classify(&self, text: &str) -> Result<GenreProfile, GenreError>;
}

/// A no-op [`GenreClassifier`] that always returns the default (myth)
/// [`GenreProfile`], regardless of input.
///
/// The only implementation until ADR-0018's extraction pipeline gives
/// classification real signal; kept as a concrete, zero-sized type so callers
/// have something to construct today without depending on a stub that lives only
/// in test code.
#[derive(Clone, Copy, Debug, Default)]
pub struct StubGenreClassifier;

#[async_trait]
impl GenreClassifier for StubGenreClassifier {
    async fn classify(&self, _text: &str) -> Result<GenreProfile, GenreError> {
        Ok(GenreProfile::default())
    }
}

#[cfg(test)]
mod tests {
    use super::{GenreClassifier, StubGenreClassifier};
    use crate::profile::GenreProfile;

    #[tokio::test]
    async fn stub_classifier_returns_the_default_myth_profile() {
        let classifier = StubGenreClassifier;
        let profile = classifier
            .classify("any input text at all")
            .await
            .expect("the stub never fails");
        assert_eq!(profile, GenreProfile::default());
    }

    #[tokio::test]
    async fn stub_classifier_ignores_its_input() {
        let classifier = StubGenreClassifier;
        let empty = classifier.classify("").await.expect("stub never fails");
        let long = classifier
            .classify("a much longer piece of narrative text, for good measure")
            .await
            .expect("stub never fails");
        assert_eq!(empty, long);
    }
}
