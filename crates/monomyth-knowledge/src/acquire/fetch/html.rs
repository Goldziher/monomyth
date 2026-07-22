//! HTML page text extraction — GET a page and pull its visible text, so a
//! plain HTTP site with no structured API (`iapsop`, `sacred_texts`,
//! `duchas`) can still be fetched through the shared transport.
//!
//! This fetches exactly one page per call (`entry.url`) — no link-following
//! crawler. A proper HTML5 parser (`scraper`/`html5ever`, plus the `ego-tree`
//! type it hands back a document as) walks the DOM rather than a
//! regex-based tag stripper, which cannot correctly skip `<script>`/`<style>`
//! content nested inside the elements a regex would otherwise match. Ports
//! no Python precedent; the retired prototype declared none of these
//! sources.

use ego_tree::NodeRef;
use scraper::{Html, Node};

use crate::acquire::error::AcquireError;
use crate::acquire::fetch::FetchedWork;
use crate::acquire::http::{self, FetchContext};

/// Element names whose entire subtree is skipped: non-visible or non-prose
/// content that would otherwise pollute the extracted text.
const SKIPPED_ELEMENTS: [&str; 4] = ["script", "style", "noscript", "head"];

/// Block-level element names after which a paragraph break is inserted, so
/// the extracted text keeps roughly the page's paragraph structure for
/// downstream chunking (the pipeline's [`super::super::normalize::clean_full_text`]
/// then collapses/re-joins on these breaks exactly as it does for any other
/// fetcher family).
const BLOCK_ELEMENTS: [&str; 9] = ["p", "div", "li", "h1", "h2", "h3", "h4", "h5", "h6"];

/// Extract the visible text of an HTML document: every text node not nested
/// under a [`SKIPPED_ELEMENTS`] element, with a paragraph break inserted
/// after each [`BLOCK_ELEMENTS`] element closes.
#[must_use]
pub(crate) fn extract_text(html: &str) -> String {
    let document = Html::parse_document(html);
    let mut out = String::new();
    collect_text(document.tree.root(), &mut out);
    out
}

/// Depth-first walk of `node`, appending visible text to `out` and skipping
/// the subtree of any [`SKIPPED_ELEMENTS`] element entirely.
fn collect_text(node: NodeRef<'_, Node>, out: &mut String) {
    if let Node::Element(element) = node.value()
        && SKIPPED_ELEMENTS.contains(&element.name())
    {
        return;
    }
    if let Node::Text(text) = node.value() {
        out.push_str(&text.text);
        out.push(' ');
    }
    for child in node.children() {
        collect_text(child, out);
    }
    if let Node::Element(element) = node.value()
        && BLOCK_ELEMENTS.contains(&element.name())
    {
        out.push_str("\n\n");
    }
}

/// GET `url`, extract its visible text, and wrap it as a [`FetchedWork`].
///
/// # Errors
///
/// Returns [`AcquireError::Http`]/[`AcquireError::Cache`] as
/// [`http::get_text`].
pub(crate) async fn fetch(
    ctx: &FetchContext<'_>,
    url: &str,
    title: Option<String>,
    retrieved: &str,
) -> Result<FetchedWork, AcquireError> {
    let (html, raw_bytes) = http::get_text(ctx, url).await?;
    let checksum = http::sha256_prefixed(&raw_bytes);
    Ok(FetchedWork {
        full_text: extract_text(&html),
        url: url.to_owned(),
        checksum,
        retrieved: retrieved.to_owned(),
        title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_text_skips_script_and_style_content() {
        let html = "<html><head><style>.x{color:red}</style><script>evil()</script></head>\
                     <body><p>Hello world.</p></body></html>";
        assert_eq!(extract_text(html), "Hello world. \n\n");
    }

    #[test]
    fn extract_text_inserts_a_paragraph_break_after_each_block_element() {
        let html = "<html><body><p>First para.</p><p>Second para.</p></body></html>";
        assert_eq!(extract_text(html), "First para. \n\nSecond para. \n\n");
    }

    #[test]
    fn extract_text_ignores_noscript_content() {
        let html =
            "<html><body><noscript>enable javascript</noscript><p>Real text.</p></body></html>";
        assert_eq!(extract_text(html), "Real text. \n\n");
    }

    /// Ties this fetcher to the shared normalize pipeline every other family
    /// goes through ([`crate::acquire::normalize_for_family`] for non-Gutendex
    /// sources): the raw, whitespace-ragged output of [`extract_text`]
    /// collapses to clean, blank-line-separated paragraphs.
    #[test]
    fn extract_text_output_cleans_up_through_the_shared_normalize_pipeline() {
        let html = "<html><head><style>.x{}</style></head><body>\
                     <p>Hello   <b>world</b>.</p><p>Second   para.</p></body></html>";
        let raw = extract_text(html);
        assert_eq!(
            crate::acquire::normalize::clean_full_text(&raw),
            "Hello world .\n\nSecond para."
        );
    }
}
