//! Shared test-only fakes for exercising the compose pipeline without a network
//! call, token spend, or ONNX embedding model.
//!
//! [`FakeBackend`] plays back a fixed script of [`Continuation`] responses in
//! order, capturing every prompt it is asked to complete; it is the single
//! source of truth for this pattern, reused by `generate`, `draft`, and
//! `compose`'s own test modules rather than duplicated in each.
//! [`in_memory_reference_knowledge`] is the compose analogue of
//! `monomyth-knowledge`'s own `test_knowledge()` helper: an in-memory vector
//! store plus a deterministic fake embedder, for exercising REFERENCE-path
//! retrieval offline.

#![cfg(test)]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use monomyth_knowledge::rag::InMemoryVectorStore;
use monomyth_knowledge::rag::pipeline::Embedder;
use monomyth_knowledge::{EMBEDDING_DIM, Knowledge, Ledger};
use monomyth_llm::{BackendError, StructuredBackend, Usage};
use serde_json::Value;

pub(crate) use crate::generate::Continuation;

/// A shared call counter, cloned out of a [`FakeBackend`] before it is moved
/// into an [`monomyth_llm::Llm`], so tests can assert exactly how many backend
/// calls a call under test made.
pub(crate) type CallCount = Arc<Mutex<usize>>;

/// A shared prompt log, cloned out of a [`FakeBackend`] before it is moved into
/// an [`monomyth_llm::Llm`], so tests can assert on what reached the model.
type PromptLog = Arc<Mutex<Vec<String>>>;

/// A fake [`StructuredBackend`] that plays back a fixed script of
/// [`Continuation`] responses in order, recording how many calls it served and
/// capturing every prompt it was asked to complete.
///
/// `complete_text` is unused by anything in this crate (which only calls
/// `Llm::generate`), so it is left unimplemented rather than scripted.
pub(crate) struct FakeBackend {
    script: Mutex<Vec<Continuation>>,
    calls: CallCount,
    prompts: PromptLog,
}

impl FakeBackend {
    /// Build a fake backend that returns `responses` in order, one per call.
    pub(crate) fn scripted(responses: Vec<Continuation>) -> Self {
        Self {
            script: Mutex::new(responses),
            calls: CallCount::default(),
            prompts: PromptLog::default(),
        }
    }

    /// A shared handle to the call counter, cloned out before the backend is
    /// moved into an [`monomyth_llm::Llm`].
    pub(crate) fn call_count(&self) -> CallCount {
        Arc::clone(&self.calls)
    }

    /// A shared handle to the prompt log, cloned out before the backend is
    /// moved into an [`monomyth_llm::Llm`].
    pub(crate) fn prompt_log(&self) -> PromptLog {
        Arc::clone(&self.prompts)
    }

    /// A snapshot of every prompt captured so far, in call order.
    pub(crate) fn prompts(log: &PromptLog) -> Vec<String> {
        log.lock().expect("lock poisoned").clone()
    }
}

#[async_trait]
impl StructuredBackend for FakeBackend {
    async fn complete_json(
        &self,
        prompt: &str,
        _schema_name: &str,
        _schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError> {
        *self.calls.lock().expect("lock poisoned") += 1;
        self.prompts
            .lock()
            .expect("lock poisoned")
            .push(prompt.to_owned());
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

/// The in-memory store name used by [`in_memory_reference_knowledge`]; arbitrary
/// but fixed, matching `monomyth-knowledge`'s own test helper.
const STORE_NAME: &str = "monomyth-compose-tests";

/// A deterministic, offline fake [`Embedder`]: a stable, content-derived vector
/// of the collection dimension. No ONNX, no network — mirrors
/// `monomyth-knowledge`'s own `FakeEmbedder` test fixture exactly, so retrieval
/// behavior in these tests matches what the knowledge crate itself verifies.
#[derive(Debug)]
struct FakeEmbedder;

#[async_trait]
impl Embedder for FakeEmbedder {
    async fn embed(&self, texts: Vec<String>) -> monomyth_knowledge::rag::RagResult<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| deterministic_vector(text))
            .collect())
    }
}

/// Deterministically map `text` to an `EMBEDDING_DIM`-wide vector by
/// accumulating each byte's value into a slot chosen by its position. Distinct
/// texts embed to distinct vectors; the same text always embeds identically.
fn deterministic_vector(text: &str) -> Vec<f32> {
    let mut vector = vec![0.0f32; EMBEDDING_DIM as usize];
    for (index, byte) in text.bytes().enumerate() {
        let slot = index % EMBEDDING_DIM as usize;
        vector[slot] += f32::from(byte) / 255.0;
    }
    vector
}

/// Build a deterministic, offline [`Knowledge`] layer over an in-memory vector
/// store and the fake embedder above, for exercising REFERENCE-path retrieval
/// in `draft`/`compose` tests without ONNX, network access, or token spend.
///
/// The compose analogue of `monomyth-knowledge`'s own `test_knowledge()`
/// helper.
pub(crate) fn in_memory_reference_knowledge() -> Knowledge {
    let store: Arc<dyn monomyth_knowledge::rag::VectorStore> =
        Arc::new(InMemoryVectorStore::new(STORE_NAME));
    let embedder: Arc<dyn Embedder> = Arc::new(FakeEmbedder);
    let ledger = Ledger::load_embedded().expect("embedded manifest parses");
    Knowledge::with(store, embedder, ledger)
}
