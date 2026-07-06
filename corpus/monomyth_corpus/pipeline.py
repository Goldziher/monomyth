"""Orchestrate a source end-to-end: select -> fetch -> normalize -> chunk -> tagged JSONL.

The record schema written per line matches the plan's Part D handoff so the Rust side
can ingest it directly (source/license/namespace/domain + provenance + slots for
entities/atu/motifs filled by later enrichment steps).
"""

from __future__ import annotations

import datetime
import json

from . import chunk, config, ledger, normalize
from .fetch import gutenberg


def _record(*, cid, work, work_id, chunk_index, text, tags, checksum, retrieved) -> dict:
    return {
        "id": cid,
        "source": tags["source"],
        "work": work,
        "work_id": work_id,
        "url": tags["url"],
        "license": tags["license"],
        "tier": tags["tier"],
        "namespace": tags["namespace"],
        "domain": tags["domain"],
        "retrieved": retrieved,
        "checksum": checksum,
        "chunk_index": chunk_index,
        "text": text,
        "tokens_est": chunk.tokens_est(text),
        "entities": [],  # QIDs — filled by entity-linking later
        "atu": None,  # tale-type tag — filled by enrichment later
        "motifs": [],  # Thompson motif codes — filled by enrichment later
    }


def run_gutenberg_topics(spec_path=None) -> dict:
    """Topic-driven fetch: resolve PD works per domain, organize output BY DOMAIN, write a catalog.

    Resilient: an individual work that fails to fetch or yields no chunks is warned and skipped,
    so one bad id never aborts the run. De-dupes works that match multiple topics.
    """
    spec_path = spec_path or (config.SOURCES / "gutenberg_topics.json")
    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    source_id = spec["source"]

    ledger.assert_shippable(source_id)
    tags = ledger.tags(source_id)
    retrieved = datetime.date.today().isoformat()
    config.TEXT_OUT.mkdir(parents=True, exist_ok=True)

    seen: set[int] = set()
    catalog: list[dict] = []
    warnings: list[str] = []
    handles: dict[str, object] = {}

    def handle(domain: str):
        if domain not in handles:
            handles[domain] = (config.TEXT_OUT / f"{domain}.jsonl").open("w", encoding="utf-8")
        return handles[domain]

    try:
        for q in spec["domains"]:
            domain = q["domain"]
            candidates = gutenberg.search_topic(
                topic=q.get("topic"), search=q.get("search"), limit=q.get("limit", 6) * 2
            )
            picked = 0
            for c in candidates:
                if picked >= q.get("limit", 6):
                    break
                if c.pg_id in seen:
                    continue
                seen.add(c.pg_id)
                try:
                    work = gutenberg.fetch(c.pg_id, c.title, domain, text_url=c.text_url)
                except Exception as exc:  # noqa: BLE001 - resilience over one bad work
                    warnings.append(f"pg:{c.pg_id} '{c.title[:40]}' fetch failed: {exc}")
                    continue
                body, stripped = normalize.strip_gutenberg(work.text)
                if not stripped:
                    warnings.append(f"pg:{c.pg_id} '{c.title[:40]}' PG markers not found")
                chunks = chunk.pack(normalize.paragraphs(body))
                if not chunks:
                    warnings.append(f"pg:{c.pg_id} '{c.title[:40]}' produced 0 chunks")
                    continue
                checksum = http_sha(work.raw_bytes)
                work_id = f"gutenberg:{c.pg_id}"
                fh = handle(domain)
                for i, text in enumerate(chunks):
                    rec = _record(
                        cid=f"{source_id}:{c.pg_id}:{i:04d}",
                        work=c.title,
                        work_id=work_id,
                        chunk_index=i,
                        text=text,
                        tags={**tags, "domain": domain},
                        checksum=checksum,
                        retrieved=retrieved,
                    )
                    fh.write(json.dumps(rec, ensure_ascii=False) + "\n")
                catalog.append(
                    {
                        "work_id": work_id,
                        "pg_id": c.pg_id,
                        "title": c.title,
                        "domain": domain,
                        "source": source_id,
                        "license": tags["license"],
                        "url": work.url,
                        "chunks": len(chunks),
                        "tokens_est": sum(chunk.tokens_est(t) for t in chunks),
                        "checksum": checksum,
                        "retrieved": retrieved,
                        "download_count": c.download_count,
                        "authors": c.authors,
                    }
                )
                picked += 1
                print(f"  {domain:10s} {work_id:16s} {len(chunks):4d} chunks  {c.title[:46]}")
    finally:
        for fh in handles.values():
            fh.close()

    (config.TEXT_OUT / "catalog.json").write_text(
        json.dumps({"generated": retrieved, "source": source_id, "works": catalog}, ensure_ascii=False, indent=2),
        encoding="utf-8",
    )
    return {
        "works": len(catalog),
        "chunks": sum(w["chunks"] for w in catalog),
        "domains": sorted(handles),
        "warnings": warnings,
    }


def run_gutenberg(spec_path=None, *, out_name: str = "gutenberg") -> dict:
    """Fetch the curated Gutenberg ship-safe works and emit artifacts/text/<out_name>.jsonl."""
    spec_path = spec_path or (config.SOURCES / "gutenberg_ship.json")
    spec = json.loads(spec_path.read_text(encoding="utf-8"))
    source_id = spec["source"]

    ledger.assert_shippable(source_id)  # hard gate before writing a ship output
    tags = ledger.tags(source_id)
    retrieved = datetime.date.today().isoformat()

    config.TEXT_OUT.mkdir(parents=True, exist_ok=True)
    out_path = config.TEXT_OUT / f"{out_name}.jsonl"

    stats = {"works": 0, "chunks": 0, "warnings": []}
    with out_path.open("w", encoding="utf-8") as fh:
        for w in spec["works"]:
            year = w.get("translator_year")
            if year is not None and year >= 1930:
                stats["warnings"].append(
                    f"pg:{w['pg_id']} '{w['title']}' translator_year={year} >= 1930 — verify US-PD before shipping"
                )
            work = gutenberg.fetch(w["pg_id"], w["title"], w.get("domain", tags["domain"]), year)
            body, stripped = normalize.strip_gutenberg(work.text)
            if not stripped:
                stats["warnings"].append(f"pg:{w['pg_id']} PG markers not found — check boilerplate")
            paras = normalize.paragraphs(body)
            chunks = chunk.pack(paras)
            checksum = http_sha(work.raw_bytes)
            work_id = f"gutenberg:{work.pg_id}"
            for i, text in enumerate(chunks):
                cid = f"{source_id}:{work.pg_id}:{i:04d}"
                rec = _record(
                    cid=cid,
                    work=work.title,
                    work_id=work_id,
                    chunk_index=i,
                    text=text,
                    tags={**tags, "domain": work.domain},
                    checksum=checksum,
                    retrieved=retrieved,
                )
                fh.write(json.dumps(rec, ensure_ascii=False) + "\n")
            stats["works"] += 1
            stats["chunks"] += len(chunks)
            print(f"  {work_id:18s} {len(chunks):4d} chunks  {work.title}")

    stats["out"] = str(out_path.relative_to(config.ROOT))
    return stats


def http_sha(data: bytes) -> str:
    from .http import sha256

    return sha256(data)
