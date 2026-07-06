"""Text normalization — strip Project Gutenberg boilerplate and unwrap paragraphs.

Stripping the PG header/footer is a licensing requirement (the trademark boilerplate
must not ship) as well as a quality one. Unwrapping the hard line-wrapping that PG
plain-text uses turns ragged 70-col lines back into real paragraphs, which chunk far
better for retrieval.
"""

from __future__ import annotations

import re

# Header end-markers (take text AFTER the last match). Covers modern *** markers
# (EBOOK/ETEXT) and the old "*END* THE SMALL PRINT! ... *END*" legal block.
_START_PATTS = (
    re.compile(r"\*\*\*\s*START OF (?:THE|THIS) PROJECT GUTENBERG E(?:BOOK|TEXT).*?\*\*\*", re.I | re.S),
    re.compile(r"\*END\*\s*THE SMALL PRINT.*?\*END\*", re.I | re.S),
)

# Footer start-markers (take text BEFORE the earliest match). Covers modern *** markers
# AND the old plain-sentence forms: "End of Project Gutenberg's <Title>, by <Author>"
# and "End of the Project Gutenberg EBook of ...".
_END_PATTS = (
    re.compile(r"\*\*\*\s*END OF (?:THE|THIS) PROJECT GUTENBERG E(?:BOOK|TEXT)", re.I),
    re.compile(r"^\s*End of (?:the )?Project Gutenberg(?:'s)?\b.*$", re.I | re.M),
)

# Conservative removal of transcription-credit lines PG places just after the START
# marker (also catches the wrapped 2nd line of a "... Online Distributed / Proofreading
# Team." credit). Very unlikely to appear in myth/folklore body text.
_CREDIT = re.compile(
    r"^.*\b(?:Produced by|Prepared by|Transcribed by|Distributed Proofreading|"
    r"Proofreading Team|Online Distributed|Transcriber'?s Notes?|Project Gutenberg)\b.*$",
    re.I | re.M,
)


def strip_gutenberg(text: str) -> tuple[str, bool]:
    """Return (body, stripped?) with PG header/footer removed (modern + legacy formats)."""
    stripped = False

    # Cut the header: after the LAST header end-marker found (past small-print + START).
    header_end = -1
    for patt in _START_PATTS:
        for m in patt.finditer(text):
            header_end = max(header_end, m.end())
    if header_end >= 0:
        text = text[header_end:]
        stripped = True

    # Cut the footer: before the EARLIEST footer start-marker found.
    footer_start = len(text)
    for patt in _END_PATTS:
        m = patt.search(text)
        if m:
            footer_start = min(footer_start, m.start())
    if footer_start < len(text):
        text = text[:footer_start]
        stripped = True

    text = _CREDIT.sub("", text)
    return text.strip(), stripped


def paragraphs(text: str) -> list[str]:
    """Split into paragraphs on blank lines and unwrap hard-wrapped lines within each."""
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    raw_paras = re.split(r"\n\s*\n", text)
    out: list[str] = []
    for para in raw_paras:
        # Join hard-wrapped lines into one; collapse runs of whitespace.
        joined = re.sub(r"\s+", " ", para.replace("\n", " ")).strip()
        if joined:
            out.append(joined)
    return out
