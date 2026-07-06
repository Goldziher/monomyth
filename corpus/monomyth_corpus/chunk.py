"""Deterministic chunking — greedy paragraph packing into bounded windows.

No randomness: the same input text always yields the same chunks and ids, so
outputs are reproducible and diffable.
"""

from __future__ import annotations

import re

from . import config


def _split_long(para: str, max_chars: int) -> list[str]:
    """Split an over-long paragraph on sentence boundaries, then hard-split if needed."""
    if len(para) <= max_chars:
        return [para]
    sentences = re.split(r"(?<=[.!?])\s+", para)
    out: list[str] = []
    buf = ""
    for s in sentences:
        if len(s) > max_chars:  # a single monster "sentence" — hard-split
            for i in range(0, len(s), max_chars):
                out.append(s[i : i + max_chars])
            continue
        if buf and len(buf) + 1 + len(s) > max_chars:
            out.append(buf)
            buf = s
        else:
            buf = f"{buf} {s}".strip()
    if buf:
        out.append(buf)
    return out


def pack(paragraphs: list[str], max_chars: int = config.CHUNK_MAX_CHARS) -> list[str]:
    """Greedily pack paragraphs into windows of at most max_chars characters."""
    chunks: list[str] = []
    buf = ""
    for para in paragraphs:
        for piece in _split_long(para, max_chars):
            if buf and len(buf) + 2 + len(piece) > max_chars:
                chunks.append(buf)
                buf = piece
            else:
                buf = f"{buf}\n\n{piece}".strip()
    if buf:
        chunks.append(buf)
    return chunks


def tokens_est(text: str) -> int:
    return max(1, len(text) // config.CHARS_PER_TOKEN)
