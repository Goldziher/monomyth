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

/// One step of the [`collect_text`] walk: visit a node (push its own text,
/// queue its children), or — for a node already visited whose children are
/// now done — close it (push a paragraph break for a block element).
enum Step<'a> {
    /// Visit `NodeRef`, pushing its own text before its children are walked.
    Enter(NodeRef<'a, Node>),
    /// `NodeRef`'s children have all been walked; push its closing text.
    Exit(NodeRef<'a, Node>),
}

/// Depth-first walk of `root`, appending visible text to `out` and skipping
/// the subtree of any [`SKIPPED_ELEMENTS`] element entirely.
///
/// Iterative (an explicit heap-allocated stack of [`Step`]s) rather than
/// recursive: a pathologically deep page (deeply nested `<div>`s, say) must
/// not risk a stack overflow the way a recursive walk bound to the native
/// call stack would.
fn collect_text(root: NodeRef<'_, Node>, out: &mut String) {
    let mut stack = vec![Step::Enter(root)];
    while let Some(step) = stack.pop() {
        match step {
            Step::Enter(node) => {
                if let Node::Element(element) = node.value()
                    && SKIPPED_ELEMENTS.contains(&element.name())
                {
                    continue;
                }
                if let Node::Text(text) = node.value() {
                    out.push_str(&text.text);
                    out.push(' ');
                }
                let is_block = matches!(
                    node.value(),
                    Node::Element(element) if BLOCK_ELEMENTS.contains(&element.name())
                );
                if is_block {
                    stack.push(Step::Exit(node));
                }
                // Children must be pushed in reverse so popping the stack ~keep
                // (LIFO) visits them in original document order. ~keep
                stack.extend(
                    node.children()
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                        .map(Step::Enter),
                );
            }
            Step::Exit(node) => {
                if let Node::Element(element) = node.value()
                    && BLOCK_ELEMENTS.contains(&element.name())
                {
                    out.push_str("\n\n");
                }
            }
        }
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

    /// Pins the point of the iterative (not recursive) walk: a pathologically
    /// deep page must not stack-overflow. A few thousand nested `<div>`s is
    /// already well past what a recursive walk risks blowing a native call
    /// stack on; the explicit heap-allocated stack in [`collect_text`]
    /// handles it. Kept in the low thousands (not tens of thousands) so this
    /// stays a fast unit test — `html5ever`'s parse cost grows steeply with
    /// nesting depth.
    #[test]
    fn extract_text_handles_a_pathologically_deep_document_without_overflowing() {
        const DEPTH: usize = 3_000;
        let mut html = String::from("<html><body>");
        html.push_str(&"<div>".repeat(DEPTH));
        html.push_str("deep text");
        html.push_str(&"</div>".repeat(DEPTH));
        html.push_str("</body></html>");

        let text = extract_text(&html);
        assert!(text.trim().starts_with("deep text"));
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
