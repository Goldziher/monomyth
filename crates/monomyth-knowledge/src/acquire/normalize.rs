//! Text normalization: strip Project Gutenberg boilerplate and unwrap
//! hard-wrapped paragraphs.
//!
//! Stripping the PG header/footer is a licensing requirement (the trademark
//! boilerplate must not ship) as well as a quality one. Unwrapping the hard
//! line-wrapping that PG plain-text uses turns ragged columns back into real
//! paragraphs, which chunk far better for retrieval. Ports the retired Python
//! prototype's `normalize.py` field-for-field.

use std::sync::LazyLock;

use regex::Regex;

/// Header end-markers: cut everything up to and including the **last** match
/// of either pattern. Covers the modern `*** START OF ... ***` marker family
/// and the legacy `*END*THE SMALL PRINT...*END*` legal block.
static START_PATTERNS: LazyLock<[Regex; 2]> = LazyLock::new(|| {
    [
        Regex::new(
            r"(?is)\*\*\*\s*START OF (?:THE|THIS) PROJECT GUTENBERG E(?:BOOK|TEXT).*?\*\*\*",
        )
        .expect("static pattern compiles"),
        Regex::new(r"(?is)\*END\*\s*THE SMALL PRINT.*?\*END\*").expect("static pattern compiles"),
    ]
});

/// Footer start-markers: cut everything from the **earliest** match of either
/// pattern to the end. Covers the modern `*** END OF ... ***` marker and the
/// legacy plain-sentence forms ("End of Project Gutenberg's ...").
static END_PATTERNS: LazyLock<[Regex; 2]> = LazyLock::new(|| {
    [
        Regex::new(r"(?i)\*\*\*\s*END OF (?:THE|THIS) PROJECT GUTENBERG E(?:BOOK|TEXT)")
            .expect("static pattern compiles"),
        Regex::new(r"(?im)^\s*End of (?:the )?Project Gutenberg(?:'s)?\b.*$")
            .expect("static pattern compiles"),
    ]
});

/// Conservative removal of transcription-credit lines PG places near the
/// header/footer. Very unlikely to appear in myth/folklore body text.
static CREDIT_LINE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)^.*\b(?:Produced by|Prepared by|Transcribed by|Distributed Proofreading|Proofreading Team|Online Distributed|Transcriber'?s Notes?|Project Gutenberg)\b.*$",
    )
    .expect("static pattern compiles")
});

/// Runs of whitespace, collapsed to a single space when unwrapping paragraphs.
static WHITESPACE_RUN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s+").expect("static pattern compiles"));

/// Blank-line paragraph boundaries.
static BLANK_LINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\n\s*\n").expect("static pattern compiles"));

/// Strip Project Gutenberg header/footer boilerplate and credit lines from
/// `text`, returning `(body, stripped)` where `stripped` reports whether any
/// header or footer marker was found and removed.
#[must_use]
pub(crate) fn strip_gutenberg(text: &str) -> (String, bool) {
    let mut body = text;
    let mut stripped = false;

    if let Some(header_end) = latest_match_end(body, &*START_PATTERNS) {
        body = &body[header_end..];
        stripped = true;
    }

    if let Some(footer_start) = earliest_match_start(body, &*END_PATTERNS) {
        body = &body[..footer_start];
        stripped = true;
    }

    let without_credits = CREDIT_LINE.replace_all(body, "");
    (without_credits.trim().to_owned(), stripped)
}

/// The byte offset just past the latest match across all `patterns`, if any
/// pattern matches at all.
fn latest_match_end(text: &str, patterns: &[Regex]) -> Option<usize> {
    patterns
        .iter()
        .flat_map(|pattern| pattern.find_iter(text))
        .map(|found| found.end())
        .max()
}

/// The byte offset of the earliest match across all `patterns`, if any
/// pattern matches at all.
fn earliest_match_start(text: &str, patterns: &[Regex]) -> Option<usize> {
    patterns
        .iter()
        .filter_map(|pattern| pattern.find(text))
        .map(|found| found.start())
        .min()
}

/// Split `text` into paragraphs on blank-line boundaries, unwrapping
/// hard-wrapped lines within each paragraph into a single line and collapsing
/// interior whitespace runs to a single space. Empty paragraphs (after
/// trimming) are dropped.
#[must_use]
pub(crate) fn paragraphs(text: &str) -> Vec<String> {
    let normalized_newlines = text.replace("\r\n", "\n").replace('\r', "\n");
    BLANK_LINE
        .split(&normalized_newlines)
        .filter_map(|raw_paragraph| {
            let unwrapped = raw_paragraph.replace('\n', " ");
            let collapsed = WHITESPACE_RUN.replace_all(&unwrapped, " ");
            let trimmed = collapsed.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        })
        .collect()
}

/// Rejoin [`paragraphs`] with a blank line between each — the clean full text
/// handed to [`crate::Knowledge::ingest`].
#[must_use]
pub(crate) fn clean_full_text(text: &str) -> String {
    paragraphs(text).join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A realistic PG 1971-era header block followed by a body and a footer.
    const MODERN_PG_TEXT: &str = "\
The Project Gutenberg eBook of Myths and Legends
This ebook is for the use of anyone anywhere in the United States and
most other parts of the world at no cost.

*** START OF THE PROJECT GUTENBERG EBOOK MYTHS AND LEGENDS ***

Produced by Jane Transcriber and the Online Distributed
Proofreading Team at https://www.pgdp.net

Once upon a time there lived a king who had three
sons, and they set out upon a great quest.

The end of the tale is not yet told.

*** END OF THE PROJECT GUTENBERG EBOOK MYTHS AND LEGENDS ***

This file should be named myths.txt
Updates are posted at www.gutenberg.org.
";

    /// A legacy "*END*THE SMALL PRINT" header block.
    const LEGACY_SMALL_PRINT_TEXT: &str = "\
Title: Old Tales
Author: A. Storyteller

**  Small Print!  **

*END*THE SMALL PRINT! FOR PUBLIC DOMAIN ETEXTS*Ver.04.29.93*END*

The dragon slept beneath the mountain for a thousand years.

End of the Project Gutenberg's Old Tales, by A. Storyteller
";

    /// A body laden with mid-text credit lines that must be stripped even
    /// without an enclosing header/footer marker pair.
    const CREDIT_LADEN_BODY: &str = "\
Prepared by the volunteers of Distributed Proofreading.
Transcribed by A. Volunteer from the 1888 edition.

Here begins the tale of the fox and the raven.
";

    #[test]
    fn strip_gutenberg_removes_modern_header_and_footer() {
        let (body, stripped) = strip_gutenberg(MODERN_PG_TEXT);
        assert!(stripped, "modern markers must be detected");
        assert_eq!(
            body,
            "Once upon a time there lived a king who had three\nsons, and they set out upon a great quest.\n\nThe end of the tale is not yet told."
        );
    }

    #[test]
    fn strip_gutenberg_removes_legacy_small_print_header_and_plain_footer() {
        let (body, stripped) = strip_gutenberg(LEGACY_SMALL_PRINT_TEXT);
        assert!(stripped, "legacy markers must be detected");
        assert_eq!(
            body,
            "The dragon slept beneath the mountain for a thousand years."
        );
    }

    #[test]
    fn strip_gutenberg_removes_credit_lines_with_no_surrounding_markers() {
        let (body, stripped) = strip_gutenberg(CREDIT_LADEN_BODY);
        assert!(
            !stripped,
            "no header/footer marker present, so stripped must be false"
        );
        assert_eq!(body, "Here begins the tale of the fox and the raven.");
    }

    #[test]
    fn strip_gutenberg_returns_unmarked_text_unchanged_but_trimmed() {
        let (body, stripped) = strip_gutenberg("  Just a plain paragraph, no markers at all.  ");
        assert!(!stripped);
        assert_eq!(body, "Just a plain paragraph, no markers at all.");
    }

    #[test]
    fn paragraphs_unwraps_hard_wrapped_lines_and_splits_on_blank_lines() {
        let text =
            "First   line\nsecond line   of\npara one.\n\nSecond para,\njust one line.\n\n\n";
        let result = paragraphs(text);
        assert_eq!(
            result,
            vec![
                "First line second line of para one.".to_owned(),
                "Second para, just one line.".to_owned(),
            ]
        );
    }

    #[test]
    fn paragraphs_normalizes_crlf_to_lf_before_splitting() {
        let text = "Line one\r\nline two\r\n\r\nSecond paragraph.\r\n";
        let result = paragraphs(text);
        assert_eq!(
            result,
            vec![
                "Line one line two".to_owned(),
                "Second paragraph.".to_owned(),
            ]
        );
    }

    #[test]
    fn paragraphs_drops_empty_paragraphs_after_trimming() {
        let text = "Real paragraph.\n\n   \n\nAnother real one.\n\n\n\n";
        let result = paragraphs(text);
        assert_eq!(
            result,
            vec!["Real paragraph.".to_owned(), "Another real one.".to_owned()]
        );
    }

    #[test]
    fn clean_full_text_rejoins_paragraphs_with_blank_lines() {
        let text = "First   line\nsecond line.\n\nSecond   para.\n";
        assert_eq!(
            clean_full_text(text),
            "First line second line.\n\nSecond para."
        );
    }

    #[test]
    fn clean_full_text_of_empty_input_is_empty() {
        assert_eq!(clean_full_text(""), "");
    }
}
