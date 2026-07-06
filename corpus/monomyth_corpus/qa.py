"""Quality assurance over the fetched corpus.

Loads artifacts/text/*.jsonl and reports, per domain and overall:
  - works / chunks / token volume and token distribution
  - tiny (<20 tok) and huge (>400 tok) chunk fractions
  - boilerplate leaks (PG trademark/license phrases that must never ship)
  - mojibake (U+FFFD) and stray control characters
  - exact-duplicate chunks (repeated TOC / front matter / refrains)
Writes artifacts/text/qa_report.json and prints a summary with FLAGS.
"""

from __future__ import annotations

import hashlib
import json
import re
import statistics
from collections import Counter, defaultdict

from . import config

# Genuine PG trademark/boilerplate strings only. (An earlier version also matched bare
# "START OF TH"/"END OF TH", which false-matched ordinary prose like "the end of the war".)
_BOILERPLATE = re.compile(
    r"Project Gutenberg|gutenberg-tm|gutenberg\.org|Literary Archive Foundation|PGLAF|"
    r"\*\*\*\s*(?:START|END) OF",
    re.I,
)
_CONTROL = re.compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f]")  # control chars except \t \n
TINY_TOK = 20
HUGE_TOK = 400


def _iter(path):
    for line in path.read_text(encoding="utf-8").splitlines():
        if line.strip():
            yield json.loads(line)


def run() -> tuple[dict, list[str]]:
    files = sorted(p for p in config.TEXT_OUT.glob("*.jsonl")) if config.TEXT_OUT.exists() else []
    by_domain: dict[str, dict] = defaultdict(
        lambda: {
            "works": set(),
            "chunks": 0,
            "tokens": 0,
            "toks": [],
            "tiny": 0,
            "huge": 0,
            "boilerplate": 0,
            "mojibake": 0,
            "control": 0,
            "empty": 0,
        }
    )
    dup_hashes: Counter = Counter()
    flags: list[str] = []

    for path in files:
        for rec in _iter(path):
            d = by_domain[rec["domain"]]
            text = rec.get("text", "")
            tok = rec.get("tokens_est", 0)
            d["works"].add(rec["work_id"])
            d["chunks"] += 1
            d["tokens"] += tok
            d["toks"].append(tok)
            if tok < TINY_TOK:
                d["tiny"] += 1
            if tok > HUGE_TOK:
                d["huge"] += 1
            if not text.strip():
                d["empty"] += 1
            if _BOILERPLATE.search(text):
                d["boilerplate"] += 1
            if "�" in text:
                d["mojibake"] += 1
            if _CONTROL.search(text):
                d["control"] += 1
            dup_hashes[hashlib.sha1(re.sub(r"\s+", " ", text.strip()).encode()).hexdigest()] += 1

    report: dict = {"domains": {}, "totals": {}}
    tot = Counter()
    all_toks: list[int] = []
    for domain, d in sorted(by_domain.items()):
        toks = d["toks"]
        all_toks += toks
        entry = {
            "works": len(d["works"]),
            "chunks": d["chunks"],
            "tokens_est": d["tokens"],
            "token_min": min(toks),
            "token_median": int(statistics.median(toks)),
            "token_mean": int(statistics.mean(toks)),
            "token_max": max(toks),
            "tiny_lt20": d["tiny"],
            "huge_gt400": d["huge"],
            "empty": d["empty"],
            "boilerplate_leaks": d["boilerplate"],
            "mojibake": d["mojibake"],
            "control_chars": d["control"],
        }
        report["domains"][domain] = entry
        for k in ("works", "chunks", "tokens_est", "boilerplate_leaks", "mojibake", "empty"):
            tot[k] += entry[k]
        if d["boilerplate"]:
            flags.append(f"{domain}: {d['boilerplate']} chunks contain PG boilerplate/trademark text")
        if d["empty"]:
            flags.append(f"{domain}: {d['empty']} empty chunks")
        if d["mojibake"]:
            flags.append(f"{domain}: {d['mojibake']} chunks with replacement chars (encoding)")

    dup_groups = {h: n for h, n in dup_hashes.items() if n > 1}
    dup_chunks = sum(n - 1 for n in dup_groups.values())
    report["duplicates"] = {"duplicate_groups": len(dup_groups), "redundant_chunks": dup_chunks}
    report["totals"] = dict(tot)
    if all_toks:
        report["totals"]["token_median"] = int(statistics.median(all_toks))
    if dup_chunks:
        flags.append(f"corpus: {dup_chunks} redundant chunks across {len(dup_groups)} duplicate groups")

    report["flags"] = flags
    (config.TEXT_OUT / "qa_report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2), encoding="utf-8")
    return report, flags
