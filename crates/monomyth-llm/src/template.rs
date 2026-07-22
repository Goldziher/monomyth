//! Named prompt templates with a stable identifier (roadmap 1g).
//!
//! A [`PromptTemplate`] pairs a stable `id` — recorded on the `llm.generate`
//! tracing span via [`crate::Llm::generate_from_template`], so a call can be
//! traced back to the named prompt that produced it — with a `{placeholder}`
//! substitution body. Substitution is a small, dependency-free `{name}` ->
//! value replacement: every placeholder left in the template after
//! substitution is a hard error, so a caller cannot silently ship a
//! half-filled prompt to the model.

use crate::error::LlmError;

/// A named prompt template: a stable `id` for tracing/provenance, plus a body
/// with `{name}`-style placeholders filled in by [`PromptTemplate::render`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromptTemplate {
    /// A stable identifier distinguishing this template from every other one,
    /// recorded on the `llm.generate` tracing span.
    pub id: &'static str,
    body: &'static str,
}

impl PromptTemplate {
    /// Construct a named template over `body`.
    #[must_use]
    pub const fn new(id: &'static str, body: &'static str) -> Self {
        Self { id, body }
    }

    /// Substitute every `{name}` placeholder in the template body with its
    /// matching value from `vars`. Substitutions are applied in order and each
    /// one replaces every remaining occurrence of `{name}`, so for a repeated
    /// `name` the first entry in `vars` wins (later entries for the same key
    /// find no placeholder left to replace).
    ///
    /// # Errors
    ///
    /// Returns [`LlmError::Template`] naming this template's `id` and the first
    /// `{placeholder}`-shaped span still present after every substitution has
    /// run — i.e. a variable the caller forgot to supply.
    pub fn render(&self, vars: &[(&str, &str)]) -> Result<String, LlmError> {
        let mut rendered = self.body.to_owned();
        for (name, value) in vars {
            rendered = rendered.replace(&format!("{{{name}}}"), value);
        }
        if let Some(placeholder) = first_unresolved_placeholder(&rendered) {
            return Err(LlmError::Template {
                template_id: self.id,
                placeholder,
            });
        }
        Ok(rendered)
    }
}

/// The first `{identifier}`-shaped span in `text` — `{` followed by one or
/// more ASCII alphanumerics/`_`, closed by `}` — or `None` if none remain.
///
/// A bare `{` that is not shaped like a placeholder (e.g. literal JSON braces
/// in the template body) is not reported.
fn first_unresolved_placeholder(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            if end > start && end < bytes.len() && bytes[end] == b'}' {
                return Some(text[start..end].to_owned());
            }
        }
        index += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::PromptTemplate;
    use crate::error::LlmError;

    #[test]
    fn substitutes_every_placeholder() {
        let template = PromptTemplate::new("greet", "Foretell {hero}'s departure to {realm}.");
        let rendered = template
            .render(&[("hero", "Gilgamesh"), ("realm", "the underworld")])
            .expect("all placeholders are supplied");
        assert_eq!(
            rendered,
            "Foretell Gilgamesh's departure to the underworld."
        );
    }

    #[test]
    fn a_missing_variable_is_a_hard_error() {
        let template = PromptTemplate::new("greet", "Foretell {hero}'s departure to {realm}.");
        let error = template
            .render(&[("hero", "Gilgamesh")])
            .expect_err("realm was never supplied");
        match error {
            LlmError::Template {
                template_id,
                placeholder,
            } => {
                assert_eq!(template_id, "greet");
                assert_eq!(placeholder, "realm");
            }
            other => panic!("expected LlmError::Template, got {other:?}"),
        }
    }

    #[test]
    fn a_repeated_key_takes_the_first_value() {
        let template = PromptTemplate::new("dup", "{x}-{x}");
        let rendered = template
            .render(&[("x", "a"), ("x", "b")])
            .expect("both occurrences resolve from the first entry");
        assert_eq!(rendered, "a-a");
    }

    #[test]
    fn literal_json_braces_in_the_body_are_not_reported_as_placeholders() {
        let template = PromptTemplate::new("json", r#"Return {{"name": "{name}"}}"#);
        let rendered = template
            .render(&[("name", "Enkidu")])
            .expect("the literal braces are not placeholder-shaped");
        assert_eq!(rendered, r#"Return {{"name": "Enkidu"}}"#);
    }

    #[test]
    fn extra_unused_variables_are_ignored() {
        let template = PromptTemplate::new("extra", "Hail {hero}.");
        let rendered = template
            .render(&[("hero", "Enkidu"), ("unused", "ignored")])
            .expect("an unused variable is harmless");
        assert_eq!(rendered, "Hail Enkidu.");
    }
}
