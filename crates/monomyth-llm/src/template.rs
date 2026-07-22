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
    /// matching value from `vars`, in a single pass over the *original* body.
    ///
    /// This is deliberately not "substitute, then substitute again on the
    /// result": a value is copied into the output verbatim and never rescanned
    /// for `{placeholder}` syntax, so a value that happens to contain literal
    /// `{`/`}` text (e.g. mythic-source prose, or a value equal to another
    /// placeholder's spelling) cannot be reinterpreted as template syntax and
    /// silently consume or corrupt another substitution. For a name that
    /// occurs more than once in `vars`, the first matching entry wins at every
    /// occurrence of that placeholder in the body.
    ///
    /// # Errors
    ///
    /// Returns [`LlmError::Template`] naming this template's `id` and the
    /// first `{placeholder}`-shaped span in the *original* body that has no
    /// matching entry in `vars` — i.e. a variable the caller forgot to supply.
    pub fn render(&self, vars: &[(&str, &str)]) -> Result<String, LlmError> {
        let body = self.body;
        let bytes = body.as_bytes();
        let mut rendered = String::with_capacity(body.len());
        // Byte index of the start of the not-yet-flushed plain-text run. ~keep
        // Slicing on this (rather than pushing individual bytes) keeps every ~keep
        // copy on a UTF-8 char boundary, since '{'/'}'/alnum/'_' are all ~keep
        // single-byte ASCII and never occur as a multi-byte sequence's ~keep
        // continuation byte. ~keep
        let mut flushed = 0;
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'{' {
                let start = index + 1;
                let mut end = start;
                while end < bytes.len()
                    && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_')
                {
                    end += 1;
                }
                if end > start && end < bytes.len() && bytes[end] == b'}' {
                    let name = &body[start..end];
                    let Some((_, value)) = vars.iter().find(|(key, _)| *key == name) else {
                        return Err(LlmError::Template {
                            template_id: self.id,
                            placeholder: name.to_owned(),
                        });
                    };
                    rendered.push_str(&body[flushed..index]);
                    rendered.push_str(value);
                    index = end + 1;
                    flushed = index;
                    continue;
                }
            }
            index += 1;
        }
        rendered.push_str(&body[flushed..]);
        Ok(rendered)
    }
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

    #[test]
    fn a_value_containing_placeholder_syntax_is_not_reinterpreted() {
        // A single-pass render must never rescan a substituted value for ~keep
        // `{placeholder}` syntax: `hero`'s value here is literally the text ~keep
        // `{realm}`, and it must survive verbatim rather than being consumed ~keep
        // by `realm`'s own substitution. ~keep
        let template = PromptTemplate::new("greet", "Foretell {hero}'s departure to {realm}.");
        let rendered = template
            .render(&[("hero", "{realm}"), ("realm", "the underworld")])
            .expect("both placeholders are supplied");
        assert_eq!(
            rendered, "Foretell {realm}'s departure to the underworld.",
            "hero's literal {{realm}} value must not be re-substituted"
        );
    }

    #[test]
    fn an_unresolved_placeholder_still_returns_a_template_error() {
        let template = PromptTemplate::new("greet", "Foretell {hero}'s departure to {realm}.");
        let error = template
            .render(&[("hero", "Gilgamesh")])
            .expect_err("realm was never supplied");
        assert!(
            matches!(error, LlmError::Template { template_id, ref placeholder }
                if template_id == "greet" && placeholder == "realm"),
            "expected LlmError::Template naming 'realm', got {error:?}"
        );
    }
}
