//! Shared test-only fakes for exercising the compose pipeline without a network
//! call or token spend.
//!
//! [`FakeBackend`] plays back a fixed script of [`Continuation`] responses in
//! order; it is the single source of truth for this pattern, reused by
//! `generate`, `draft`, and `compose`'s own test modules rather than duplicated
//! in each.

#![cfg(test)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use monomyth_llm::{BackendError, StructuredBackend, Usage};
use serde_json::Value;

pub(crate) use crate::generate::Continuation;

/// A shared call counter, cloned out of a [`FakeBackend`] before it is moved
/// into an [`monomyth_llm::Llm`], so tests can assert exactly how many backend
/// calls a call under test made.
pub(crate) type CallCount = Arc<Mutex<usize>>;

/// A fake [`StructuredBackend`] that plays back a fixed script of
/// [`Continuation`] responses in order, recording how many calls it served.
///
/// `complete_text` is unused by anything in this crate (which only calls
/// `Llm::generate`), so it is left unimplemented rather than scripted.
pub(crate) struct FakeBackend {
    script: Mutex<Vec<Continuation>>,
    calls: CallCount,
}

impl FakeBackend {
    /// Build a fake backend that returns `responses` in order, one per call.
    pub(crate) fn scripted(responses: Vec<Continuation>) -> Self {
        Self {
            script: Mutex::new(responses),
            calls: CallCount::default(),
        }
    }

    /// A shared handle to the call counter, cloned out before the backend is
    /// moved into an [`monomyth_llm::Llm`].
    pub(crate) fn call_count(&self) -> CallCount {
        Arc::clone(&self.calls)
    }
}

#[async_trait]
impl StructuredBackend for FakeBackend {
    async fn complete_json(
        &self,
        _prompt: &str,
        _schema_name: &str,
        _schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError> {
        *self.calls.lock().expect("lock poisoned") += 1;
        let mut script = self.script.lock().expect("lock poisoned");
        if script.is_empty() {
            return Err(BackendError::new("no scripted responses remain"));
        }
        let next = script.remove(0);
        Ok((
            serde_json::to_value(next).expect("Continuation serializes"),
            None,
        ))
    }

    async fn complete_text(&self, _prompt: &str) -> Result<(String, Option<Usage>), BackendError> {
        unimplemented!("the compose pipeline only calls Llm::generate, never Llm::text")
    }
}

/// Wrap `backend` in an [`monomyth_llm::Llm`].
pub(crate) fn llm_with(backend: FakeBackend) -> monomyth_llm::Llm {
    monomyth_llm::Llm::new(Box::new(backend))
}
