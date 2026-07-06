"""Project Gutenberg fetcher — discover PD works by topic and fetch clean plain text.

Two entry points:
  search_topic(...)  -> resolve a topic/search query to PD candidates via Gutendex
  fetch(...)         -> download one work's UTF-8 plain text + provenance

`copyright=false` on Gutendex is PG's own US-public-domain determination, so it is our
ship-safe gate: we only ever surface works PG marks non-copyright.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from urllib.parse import urlencode

from .. import http


@dataclass
class Candidate:
    pg_id: int
    title: str
    download_count: int
    text_url: str
    authors: list[str] = field(default_factory=list)


@dataclass
class Work:
    pg_id: int
    title: str
    domain: str
    text: str
    raw_bytes: bytes
    url: str


def _text_url(formats: dict) -> str | None:
    for mime, url in formats.items():
        if mime.startswith("text/plain") and not url.endswith(".zip"):
            return url
    return None


def search_topic(
    *, topic: str | None = None, search: str | None = None, limit: int = 8, languages: str = "en"
) -> list[Candidate]:
    """Return up to `limit` PD (copyright=false) candidates for a topic/search query.

    Gutendex default ordering is by download_count desc, so popularity ~ a quality proxy.
    """
    params = {"languages": languages, "copyright": "false"}
    if topic:
        params["topic"] = topic
    if search:
        params["search"] = search
    url = "https://gutendex.com/books?" + urlencode(params)

    out: list[Candidate] = []
    pages = 0
    while url and len(out) < limit and pages < 5:
        data = http.get_json(url)
        pages += 1
        for r in data.get("results", []):
            if r.get("copyright") is not False:  # strict: only explicit PD
                continue
            turl = _text_url(r.get("formats", {}))
            if not turl:
                continue
            out.append(
                Candidate(
                    pg_id=r["id"],
                    title=r["title"],
                    download_count=r.get("download_count", 0),
                    text_url=turl,
                    authors=[a.get("name", "") for a in r.get("authors", [])],
                )
            )
            if len(out) >= limit:
                break
        url = data.get("next")
    return out


def fetch(pg_id: int, title: str, domain: str, *, text_url: str | None = None) -> Work:
    url = text_url or f"https://www.gutenberg.org/cache/epub/{pg_id}/pg{pg_id}.txt"
    text, raw = http.get_text(url)
    return Work(pg_id=pg_id, title=title, domain=domain, text=text, raw_bytes=raw, url=url)
