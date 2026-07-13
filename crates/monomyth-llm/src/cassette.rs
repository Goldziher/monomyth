//! Record/replay "cassette" backend for offline, deterministic tests and CI.
//!
//! [`RecordingBackend`] wraps a live [`StructuredBackend`] and transcribes every
//! `complete_json` call (including repair retries) to a JSON cassette file.
//! [`ReplayBackend`] loads that cassette and answers calls from it — no network,
//! no provider key, byte-identical output across runs. Interactions are keyed by
//! `(schema_name, prompt)` and consumed FIFO, so a test that issues the same
//! prompt twice replays the two recorded responses in the order they were made.
//!
//! Cassettes are plain JSON, sorted-key and pretty-printed, so a diff on one is
//! readable in code review. Only the prompt and response ever reach the file —
//! no config, no provider key — so a cassette is safe to commit.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::backend::{StructuredBackend, Usage};
use crate::error::{BackendError, CassetteError};

/// FNV-1a 64-bit offset basis, per the reference algorithm.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64-bit prime, per the reference algorithm.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// The `recorded_at` value used when no explicit timestamp is supplied.
///
/// Cassette recording must stay deterministic and free of wall-clock reads, so
/// callers that don't care about the timestamp get this fixed placeholder
/// rather than a call to `SystemTime::now`.
const UNKNOWN_RECORDED_AT: &str = "unknown";

/// The cassette format version this module reads and writes.
const CASSETTE_VERSION: u32 = 1;

/// FNV-1a over `bytes`, dependency-free and stable across platforms.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Derive the lookup key for a `(schema_name, prompt)` pair.
///
/// The key is the lowercase hex FNV-1a hash of `schema_name`, a unit separator
/// (`\u{1f}`), and `prompt`. FNV-1a keeps the key short; [`Interaction::prompt_sha`]
/// carries the full sha256 alongside it so a human can audit a hash collision.
fn interaction_key(schema_name: &str, prompt: &str) -> String {
    let mut buffer = String::with_capacity(schema_name.len() + 1 + prompt.len());
    buffer.push_str(schema_name);
    buffer.push('\u{1f}');
    buffer.push_str(prompt);
    format!("{:016x}", fnv1a(buffer.as_bytes()))
}

/// The full sha256 hex digest of `prompt`, for human audit of key collisions.
fn prompt_sha256(prompt: &str) -> String {
    let digest = Sha256::digest(prompt.as_bytes());
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// One recorded `complete_json` call and its response.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Interaction {
    /// The lookup key derived from `schema_name` and the prompt; see
    /// [`interaction_key`].
    pub key: String,
    /// The schema name the call was made with.
    pub schema_name: String,
    /// The full sha256 hex digest of the prompt, for human audit of collisions.
    pub prompt_sha: String,
    /// The recorded JSON response.
    pub response: Value,
    /// The recorded usage, when the backend reported one.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub usage: Option<Usage>,
}

/// A recorded sequence of LLM interactions, replayable offline.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cassette {
    /// The cassette format version.
    pub version: u32,
    /// The `"provider/model"` routing string the interactions were recorded against.
    pub model: String,
    /// A caller-supplied timestamp for when the cassette was recorded. Fixed and
    /// deterministic — never a live clock read; see [`UNKNOWN_RECORDED_AT`].
    pub recorded_at: String,
    /// The recorded interactions, in call order.
    pub interactions: Vec<Interaction>,
}

impl Cassette {
    /// Load a cassette from `path`.
    ///
    /// # Errors
    ///
    /// Returns [`CassetteError::Io`] if the file cannot be read, or
    /// [`CassetteError::Parse`] if its contents are not a valid [`Cassette`].
    pub fn load(path: impl AsRef<Path>) -> Result<Self, CassetteError> {
        let path = path.as_ref();
        let raw = fs::read_to_string(path).map_err(|source| CassetteError::Io {
            path: path.display().to_string(),
            source,
        })?;
        serde_json::from_str(&raw).map_err(|source| CassetteError::Parse {
            path: path.display().to_string(),
            source,
        })
    }

    /// Render the cassette as sorted-key, pretty-printed JSON with a trailing
    /// newline, so a diff on a committed cassette is minimal and readable.
    ///
    /// # Errors
    ///
    /// Returns a [`serde_json::Error`] if the cassette cannot be serialized (it
    /// always can, in practice, since every field type here is serializable).
    fn to_pretty_string(&self) -> Result<String, serde_json::Error> {
        // Route through `serde_json::Value` so map keys sort lexicographically ~keep
        // regardless of struct field declaration order. ~keep
        let value: Value = serde_json::to_value(self)?;
        let mut rendered = serde_json::to_string_pretty(&sort_keys(value))?;
        rendered.push('\n');
        Ok(rendered)
    }

    /// Write the cassette to `path`, creating or overwriting it.
    ///
    /// # Errors
    ///
    /// Returns [`CassetteError::Io`] if serialization or the file write fails.
    fn save(&self, path: &Path) -> Result<(), CassetteError> {
        let rendered = self
            .to_pretty_string()
            .map_err(|source| CassetteError::Io {
                path: path.display().to_string(),
                source: std::io::Error::other(source),
            })?;
        fs::write(path, rendered).map_err(|source| CassetteError::Io {
            path: path.display().to_string(),
            source,
        })
    }
}

/// Recursively sort object keys so serialization is stable regardless of
/// struct field order or `HashMap` iteration order.
fn sort_keys(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let sorted: std::collections::BTreeMap<String, Value> = map
                .into_iter()
                .map(|(key, val)| (key, sort_keys(val)))
                .collect();
            let mut object = serde_json::Map::new();
            for (key, val) in sorted {
                object.insert(key, val);
            }
            Value::Object(object)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_keys).collect()),
        other => other,
    }
}

/// An offline [`StructuredBackend`] that answers calls from a pre-recorded
/// [`Cassette`] instead of a live provider.
///
/// This is the default backend for CI and any test that must not touch the
/// network. Interactions are consumed FIFO per `(schema_name, prompt)` key, so
/// repeated identical prompts (e.g. a retry-repair loop) replay in the order
/// they were recorded.
#[derive(Debug)]
pub struct ReplayBackend {
    interactions: Vec<Interaction>,
    /// Per-key cursor into `interactions`, tracking how many matches for a
    /// given key have already been consumed.
    cursors: Mutex<HashMap<String, usize>>,
}

impl ReplayBackend {
    /// Load a cassette from `path` and build a [`ReplayBackend`] over it.
    ///
    /// # Errors
    ///
    /// Returns [`CassetteError`] if the cassette cannot be read or parsed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use monomyth_llm::ReplayBackend;
    ///
    /// # fn run() -> Result<(), monomyth_llm::CassetteError> {
    /// let backend = ReplayBackend::load("tests/fixtures/omen.cassette.json")?;
    /// # let _ = backend;
    /// # Ok(())
    /// # }
    /// ```
    pub fn load(path: impl AsRef<Path>) -> Result<Self, CassetteError> {
        Ok(Self::from_cassette(Cassette::load(path)?))
    }

    /// Build a [`ReplayBackend`] over an already-loaded [`Cassette`].
    #[must_use]
    pub fn from_cassette(cassette: Cassette) -> Self {
        Self {
            interactions: cassette.interactions,
            cursors: Mutex::new(HashMap::new()),
        }
    }

    /// Find the next unconsumed interaction for `key`, advancing its cursor.
    fn next_for_key(&self, key: &str) -> Option<&Interaction> {
        let mut cursors = self
            .cursors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let start = cursors.get(key).copied().unwrap_or(0);
        let mut seen = 0usize;
        for interaction in &self.interactions {
            if interaction.key != key {
                continue;
            }
            if seen == start {
                cursors.insert(key.to_owned(), start + 1);
                return Some(interaction);
            }
            seen += 1;
        }
        None
    }
}

#[async_trait]
impl StructuredBackend for ReplayBackend {
    async fn complete_json(
        &self,
        prompt: &str,
        schema_name: &str,
        _schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError> {
        let key = interaction_key(schema_name, prompt);
        let Some(interaction) = self.next_for_key(&key) else {
            let error = CassetteError::Miss {
                schema_name: schema_name.to_owned(),
                prompt_sha: prompt_sha256(prompt),
            };
            return Err(BackendError::new(error.to_string()));
        };
        Ok((interaction.response.clone(), interaction.usage.clone()))
    }

    async fn complete_text(&self, _prompt: &str) -> Result<(String, Option<Usage>), BackendError> {
        Err(BackendError::new(
            "text completion is not recorded in cassettes",
        ))
    }
}

/// A [`StructuredBackend`] wrapper that transcribes every `complete_json` call
/// (including repair retries) to a cassette on disk.
///
/// Wraps a live backend `B` — typically [`crate::XbergBackend`] — and forwards
/// every call to it unchanged. On success, the call is recorded before the
/// result is returned to the caller. `complete_text` is forwarded but never
/// recorded, since only JSON completions are cassette-replayable.
pub struct RecordingBackend<B: StructuredBackend> {
    inner: B,
    model: String,
    path: PathBuf,
    recorded_at: String,
    interactions: Mutex<Vec<Interaction>>,
}

impl<B: StructuredBackend> std::fmt::Debug for RecordingBackend<B> {
    /// Deliberately opaque on `inner`: the wrapped backend may carry provider
    /// secrets (see [`crate::XbergBackend`]'s own opaque `Debug`), so this must
    /// never format it.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RecordingBackend")
            .field("model", &self.model)
            .field("path", &self.path)
            .field("recorded_at", &self.recorded_at)
            .finish_non_exhaustive()
    }
}

impl<B: StructuredBackend> RecordingBackend<B> {
    /// Wrap `inner`, recording to a cassette at `path` tagged with `model` and
    /// [`UNKNOWN_RECORDED_AT`].
    ///
    /// Use [`RecordingBackend::with_recorded_at`] to stamp a specific,
    /// caller-chosen timestamp instead.
    #[must_use]
    pub fn new(inner: B, model: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self::with_recorded_at(inner, model, path, UNKNOWN_RECORDED_AT)
    }

    /// Wrap `inner`, recording to a cassette at `path` tagged with `model` and
    /// an explicit `recorded_at` string.
    ///
    /// `recorded_at` is caller-supplied rather than sampled from the system
    /// clock, so recording stays deterministic and reproducible in tests.
    #[must_use]
    pub fn with_recorded_at(
        inner: B,
        model: impl Into<String>,
        path: impl Into<PathBuf>,
        recorded_at: impl Into<String>,
    ) -> Self {
        Self {
            inner,
            model: model.into(),
            path: path.into(),
            recorded_at: recorded_at.into(),
            interactions: Mutex::new(Vec::new()),
        }
    }

    /// Write the interactions recorded so far to the cassette path.
    ///
    /// Called automatically on drop; exposed so a caller can flush explicitly
    /// (and observe IO errors) before the backend is dropped.
    ///
    /// # Errors
    ///
    /// Returns [`CassetteError::Io`] if the cassette cannot be written.
    pub fn flush(&self) -> Result<(), CassetteError> {
        let interactions = self
            .interactions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let cassette = Cassette {
            version: CASSETTE_VERSION,
            model: self.model.clone(),
            recorded_at: self.recorded_at.clone(),
            interactions,
        };
        cassette.save(&self.path)
    }
}

#[async_trait]
impl<B: StructuredBackend> StructuredBackend for RecordingBackend<B> {
    async fn complete_json(
        &self,
        prompt: &str,
        schema_name: &str,
        schema: &Value,
    ) -> Result<(Value, Option<Usage>), BackendError> {
        let (response, usage) = self
            .inner
            .complete_json(prompt, schema_name, schema)
            .await?;
        let interaction = Interaction {
            key: interaction_key(schema_name, prompt),
            schema_name: schema_name.to_owned(),
            prompt_sha: prompt_sha256(prompt),
            response: response.clone(),
            usage: usage.clone(),
        };
        self.interactions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(interaction);
        Ok((response, usage))
    }

    async fn complete_text(&self, prompt: &str) -> Result<(String, Option<Usage>), BackendError> {
        self.inner.complete_text(prompt).await
    }
}

impl<B: StructuredBackend> Drop for RecordingBackend<B> {
    fn drop(&mut self) {
        let is_empty = self
            .interactions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty();
        if is_empty {
            return;
        }
        if let Err(error) = self.flush() {
            eprintln!("monomyth-llm: failed to flush cassette on drop: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex as StdMutex;

    use async_trait::async_trait;
    use serde_json::json;
    use tempfile::NamedTempFile;

    use super::{Cassette, Interaction, RecordingBackend, ReplayBackend, interaction_key};
    use crate::backend::{StructuredBackend, Usage};
    use crate::error::BackendError;

    /// A scripted live backend: `complete_json` pops the next canned
    /// `(response, usage)` pair per call.
    struct FakeLiveBackend {
        responses: StdMutex<Vec<(Value, Option<Usage>)>>,
    }

    type Value = serde_json::Value;

    impl FakeLiveBackend {
        fn new(responses: Vec<(Value, Option<Usage>)>) -> Self {
            Self {
                responses: StdMutex::new(responses),
            }
        }
    }

    #[async_trait]
    impl StructuredBackend for FakeLiveBackend {
        async fn complete_json(
            &self,
            _prompt: &str,
            _schema_name: &str,
            _schema: &Value,
        ) -> Result<(Value, Option<Usage>), BackendError> {
            let mut responses = self.responses.lock().expect("lock poisoned");
            if responses.is_empty() {
                return Err(BackendError::new("no scripted responses remain"));
            }
            Ok(responses.remove(0))
        }

        async fn complete_text(
            &self,
            _prompt: &str,
        ) -> Result<(String, Option<Usage>), BackendError> {
            Ok(("unused".to_owned(), None))
        }
    }

    fn usage(total: u64) -> Usage {
        Usage {
            prompt_tokens: Some(total / 2),
            completion_tokens: Some(total - total / 2),
            total_tokens: Some(total),
        }
    }

    #[tokio::test]
    async fn should_round_trip_record_then_replay() {
        let file = NamedTempFile::new().expect("create tempfile");
        let path = file.path().to_path_buf();
        let live = FakeLiveBackend::new(vec![
            (json!({"name": "Gilgamesh"}), Some(usage(10))),
            (json!({"name": "Enkidu"}), Some(usage(20))),
        ]);

        {
            let recorder = RecordingBackend::new(live, "test/stub-model", &path);
            let (first, first_usage) = recorder
                .complete_json("forge a hero", "hero", &json!({}))
                .await
                .expect("first call should succeed");
            assert_eq!(first, json!({"name": "Gilgamesh"}));
            assert_eq!(first_usage, Some(usage(10)));

            let (second, second_usage) = recorder
                .complete_json("forge a sidekick", "hero", &json!({}))
                .await
                .expect("second call should succeed");
            assert_eq!(second, json!({"name": "Enkidu"}));
            assert_eq!(second_usage, Some(usage(20)));

            recorder.flush().expect("flush should succeed");
        }

        let replay = ReplayBackend::load(&path).expect("cassette should load");

        let (replayed_first, replayed_first_usage) = replay
            .complete_json("forge a hero", "hero", &json!({}))
            .await
            .expect("replay of first prompt should succeed");
        assert_eq!(replayed_first, json!({"name": "Gilgamesh"}));
        assert_eq!(replayed_first_usage, Some(usage(10)));

        let (replayed_second, replayed_second_usage) = replay
            .complete_json("forge a sidekick", "hero", &json!({}))
            .await
            .expect("replay of second prompt should succeed");
        assert_eq!(replayed_second, json!({"name": "Enkidu"}));
        assert_eq!(replayed_second_usage, Some(usage(20)));
    }

    #[tokio::test]
    async fn should_replay_identical_key_calls_fifo_then_miss_on_a_third() {
        let file = NamedTempFile::new().expect("create tempfile");
        let path = file.path().to_path_buf();
        let live = FakeLiveBackend::new(vec![
            (json!({"attempt": 1}), None),
            (json!({"attempt": 2}), None),
        ]);

        {
            let recorder = RecordingBackend::new(live, "test/stub-model", &path);
            recorder
                .complete_json("same prompt", "hero", &json!({}))
                .await
                .expect("first attempt should succeed");
            recorder
                .complete_json("same prompt", "hero", &json!({}))
                .await
                .expect("repair retry should succeed");
            recorder.flush().expect("flush should succeed");
        }

        let replay = ReplayBackend::load(&path).expect("cassette should load");

        let (first, _) = replay
            .complete_json("same prompt", "hero", &json!({}))
            .await
            .expect("first replay should succeed");
        assert_eq!(
            first,
            json!({"attempt": 1}),
            "must replay in recorded order"
        );

        let (second, _) = replay
            .complete_json("same prompt", "hero", &json!({}))
            .await
            .expect("second replay should succeed");
        assert_eq!(
            second,
            json!({"attempt": 2}),
            "must consume FIFO, not repeat"
        );

        let miss = replay
            .complete_json("same prompt", "hero", &json!({}))
            .await
            .expect_err("a third replay of an exhausted key must miss");
        assert!(
            miss.to_string().contains("re-record"),
            "miss message must point at re-recording, got: {miss}",
        );
    }

    #[tokio::test]
    async fn should_error_loudly_on_an_unrecorded_prompt() {
        let cassette = Cassette {
            version: 1,
            model: "test/stub-model".to_owned(),
            recorded_at: "unknown".to_owned(),
            interactions: vec![Interaction {
                key: interaction_key("hero", "recorded prompt"),
                schema_name: "hero".to_owned(),
                prompt_sha: "irrelevant".to_owned(),
                response: json!({"name": "Gilgamesh"}),
                usage: None,
            }],
        };
        let replay = ReplayBackend::from_cassette(cassette);

        let error = replay
            .complete_json("a prompt that was never recorded", "hero", &json!({}))
            .await
            .expect_err("an unrecorded prompt must miss");

        let message = error.to_string();
        assert!(
            message.contains("cassette miss"),
            "message must say 'cassette miss', got: {message}"
        );
        assert!(
            message.contains("hero"),
            "message must name the schema, got: {message}"
        );
        assert!(
            message.contains("re-record with MONOMYTH_RECORD=1"),
            "message must instruct how to re-record, got: {message}"
        );
    }

    #[tokio::test]
    async fn should_never_serialize_secrets_into_a_cassette() {
        // The recorder's inputs are only ever a prompt and a `StructuredBackend` ~keep
        // response — it has no access to `LlmConfig` (which may carry a provider ~keep
        // API key) and cannot serialize what it cannot see. This test pins that ~keep
        // by construction: a normal, secret-free prompt/response round-trips ~keep
        // into a cassette that contains none of the well-known secret markers. ~keep
        let file = NamedTempFile::new().expect("create tempfile");
        let path = file.path().to_path_buf();
        let live = FakeLiveBackend::new(vec![(json!({"name": "Gilgamesh"}), None)]);

        {
            let recorder = RecordingBackend::new(live, "test/stub-model", &path);
            recorder
                .complete_json("forge a hero", "hero", &json!({}))
                .await
                .expect("call should succeed");
            recorder.flush().expect("flush should succeed");
        }

        let serialized = std::fs::read_to_string(&path).expect("read cassette");
        for marker in ["api_key", "sk-", "AIza"] {
            assert!(
                !serialized.contains(marker),
                "cassette must never contain the secret marker '{marker}', got: {serialized}"
            );
        }
    }

    #[test]
    fn should_derive_a_stable_key_that_differs_by_schema_or_prompt() {
        let base = interaction_key("hero", "forge a hero");
        let same_again = interaction_key("hero", "forge a hero");
        let different_schema = interaction_key("sidekick", "forge a hero");
        let different_prompt = interaction_key("hero", "forge a villain");

        assert_eq!(base, same_again, "key must be deterministic");
        assert_ne!(
            base, different_schema,
            "key must differ when schema_name differs"
        );
        assert_ne!(
            base, different_prompt,
            "key must differ when prompt differs"
        );
    }

    #[test]
    fn should_pretty_print_a_cassette_with_sorted_keys_and_trailing_newline() {
        let cassette = Cassette {
            version: 1,
            model: "test/stub-model".to_owned(),
            recorded_at: "unknown".to_owned(),
            interactions: vec![Interaction {
                key: "abc".to_owned(),
                schema_name: "hero".to_owned(),
                prompt_sha: "deadbeef".to_owned(),
                response: json!({"z": 1, "a": 2}),
                usage: None,
            }],
        };

        let rendered = cassette.to_pretty_string().expect("must serialize");
        assert!(rendered.ends_with('\n'), "must end with a trailing newline");
        let z_index = rendered.find("\"z\"").expect("z key present");
        let a_index = rendered.find("\"a\"").expect("a key present");
        assert!(
            a_index < z_index,
            "object keys must be sorted lexicographically"
        );
    }
}
