//! Opt-in ingest-time enrichment knobs: keyword extraction and named-entity
//! detection, both disabled by default.

use std::fmt;
#[cfg(feature = "rag-ner-llm")]
use std::sync::Arc;

/// Entity category name for a mythological or divine figure, used to seed
/// [`default_mythic_categories`].
#[cfg(feature = "rag-ner-llm")]
const MYTHIC_CATEGORY_DEITY: &str = "Deity";
/// Entity category name for a story's protagonist figure.
#[cfg(feature = "rag-ner-llm")]
const MYTHIC_CATEGORY_HERO: &str = "Hero";
/// Entity category name for a named magical or narratively significant object.
#[cfg(feature = "rag-ner-llm")]
const MYTHIC_CATEGORY_ARTIFACT: &str = "Artifact";
/// Entity category name for a named place of mythic or narrative significance.
#[cfg(feature = "rag-ner-llm")]
const MYTHIC_CATEGORY_REALM: &str = "Realm";

/// Runtime knob for best-effort keyword extraction at ingest time.
///
/// Interim home: this is a small runtime setter on [`Knowledge`] rather than a
/// `monomyth.toml` section, pending config wiring in a later slice. Defaults to
/// **disabled**, so default behavior — and every existing cassette/golden — is
/// byte-for-byte unchanged; a caller opts in via
/// [`Knowledge::with_keyword_enrichment`].
///
/// The `config` field (and hence `xberg::KeywordConfig`, which only exists
/// when `xberg/keywords` is compiled) is only present when the `rag-keywords`
/// feature is enabled. With the feature off, `enabled` can still be set but
/// extraction is compiled out entirely, so ingest always proceeds with empty
/// keywords.
#[derive(Debug, Clone, Default)]
pub struct KeywordEnrichment {
    /// Whether ingest should attempt keyword extraction.
    pub enabled: bool,
    /// Extraction configuration, forwarded to `xberg::keywords::extract_keywords`.
    #[cfg(feature = "rag-keywords")]
    pub config: xberg::KeywordConfig,
}

impl KeywordEnrichment {
    /// Enrichment enabled with the default [`xberg::KeywordConfig`].
    #[must_use]
    #[cfg(feature = "rag-keywords")]
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            config: xberg::KeywordConfig::default(),
        }
    }
}

/// Runtime knob for best-effort named-entity extraction at ingest time (WS-C
/// slice 3), mirroring [`KeywordEnrichment`]. Defaults to **disabled**, so
/// default behavior — and every existing cassette/golden — is byte-for-byte
/// unchanged; a caller opts in via [`Knowledge::with_entity_enrichment`].
///
/// The backend and categories (and hence `xberg::NerBackend` /
/// `xberg::EntityCategory`, which only exist when `xberg/ner-llm` is compiled)
/// are only present when the `rag-ner-llm` feature is enabled. With the
/// feature off, `enabled` can still be set but extraction is compiled out
/// entirely, so ingest always proceeds with no entities.
#[derive(Clone, Default)]
pub struct EntityEnrichment {
    /// Whether ingest should attempt named-entity extraction.
    pub enabled: bool,
    /// The NER backend to call, forwarded to `xberg::detect_entities`.
    #[cfg(feature = "rag-ner-llm")]
    pub(crate) backend: Option<Arc<dyn xberg::NerBackend>>,
    /// The entity categories to detect.
    #[cfg(feature = "rag-ner-llm")]
    pub(crate) categories: Vec<xberg::EntityCategory>,
}

impl fmt::Debug for EntityEnrichment {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug_struct = formatter.debug_struct("EntityEnrichment");
        debug_struct.field("enabled", &self.enabled);
        #[cfg(feature = "rag-ner-llm")]
        debug_struct.field("has_backend", &self.backend.is_some());
        // `categories` is intentionally omitted (not useful in a log line); ~keep
        // `finish_non_exhaustive` documents that this Debug impl is a summary, ~keep
        // not a full field dump, satisfying `clippy::missing_fields_in_debug`. ~keep
        debug_struct.finish_non_exhaustive()
    }
}

impl EntityEnrichment {
    /// Enrichment enabled with an explicit backend and category set.
    ///
    /// The narrow seam a caller (or test) uses to supply a fake backend
    /// without a live LLM call.
    #[must_use]
    #[cfg(feature = "rag-ner-llm")]
    pub fn with_backend(
        backend: Arc<dyn xberg::NerBackend>,
        categories: Vec<xberg::EntityCategory>,
    ) -> Self {
        Self {
            enabled: true,
            backend: Some(backend),
            categories,
        }
    }

    /// Enrichment enabled with an `xberg::LlmBackend` built from `config`,
    /// detecting the [`default_mythic_categories`] — the categories a mythic
    /// corpus actually needs, rather than the generic PERSON/ORG/LOCATION set.
    #[must_use]
    #[cfg(feature = "rag-ner-llm")]
    pub fn llm(config: xberg::LlmConfig) -> Self {
        let backend = Arc::new(xberg::LlmBackend::new(config)) as Arc<dyn xberg::NerBackend>;
        Self::with_backend(backend, default_mythic_categories())
    }
}

/// The mythic-corpus entity categories an [`EntityEnrichment::llm`] backend
/// detects by default: figures, protagonists, artifacts, and realms — the
/// entity shapes generation priors actually care about, distinct from the
/// generic PERSON/ORGANIZATION/LOCATION taxonomy built-in categories cover.
#[cfg(feature = "rag-ner-llm")]
fn default_mythic_categories() -> Vec<xberg::EntityCategory> {
    vec![
        xberg::EntityCategory::Custom(MYTHIC_CATEGORY_DEITY.to_owned()),
        xberg::EntityCategory::Custom(MYTHIC_CATEGORY_HERO.to_owned()),
        xberg::EntityCategory::Custom(MYTHIC_CATEGORY_ARTIFACT.to_owned()),
        xberg::EntityCategory::Custom(MYTHIC_CATEGORY_REALM.to_owned()),
    ]
}
