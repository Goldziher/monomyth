"""CLI: python -m monomyth_corpus <command>

Commands:
  fetch gutenberg    Fetch the curated ship-safe PG works -> artifacts/text/gutenberg.jsonl
  verify             Re-check all artifacts/text/*.jsonl (schema + license invariant)
  stats              Summarize produced chunks by source / domain / namespace
"""

from __future__ import annotations

import argparse
import json
import sys
from collections import Counter

from . import config, pipeline, qa, verify


def _cmd_fetch(args) -> int:
    if args.source == "gutenberg":
        print("fetch gutenberg (by topic, copyright=false):")
        stats = pipeline.run_gutenberg_topics()
        print(
            f"\n  {stats['works']} works, {stats['chunks']} chunks across "
            f"{len(stats['domains'])} domains -> artifacts/text/<domain>.jsonl + catalog.json"
        )
        for w in stats["warnings"]:
            print(f"  warn: {w}")
        return 0
    print(f"unknown source: {args.source}", file=sys.stderr)
    return 2


def _cmd_qa(_args) -> int:
    print("qa report (artifacts/text):")
    report, flags = qa.run()
    hdr = f"  {'domain':11s} {'works':>5s} {'chunks':>7s} {'tokens':>9s} {'med':>4s} {'tiny':>5s} {'huge':>5s}"
    print(hdr)
    for domain, e in report["domains"].items():
        print(
            f"  {domain:11s} {e['works']:5d} {e['chunks']:7d} {e['tokens_est']:9d} "
            f"{e['token_median']:4d} {e['tiny_lt20']:5d} {e['huge_gt400']:5d}"
        )
    t = report["totals"]
    dup = report["duplicates"]
    print(f"  {'TOTAL':11s} {t.get('works', 0):5d} {t.get('chunks', 0):7d} {t.get('tokens_est', 0):9d}")
    print(f"\n  duplicates: {dup['redundant_chunks']} redundant chunks / {dup['duplicate_groups']} groups")
    print(
        f"  boilerplate leaks: {t.get('boilerplate_leaks', 0)}  mojibake: {t.get('mojibake', 0)}  empty: {t.get('empty', 0)}"
    )
    if flags:
        print("\n  FLAGS:")
        for f in flags:
            print(f"   - {f}")
    else:
        print("\n  no quality flags.")
    print("\n  wrote artifacts/text/qa_report.json")
    return 0


def _cmd_verify(_args) -> int:
    print("verify artifacts/text:")
    n, errors = verify.verify_all()
    if not n:
        print("  (no jsonl files yet)")
    if errors:
        print("\nFAILED:")
        for e in errors:
            print(f"  - {e}")
        return 1
    print("\nAll text artifacts pass schema + license invariant.")
    return 0


def _cmd_stats(_args) -> int:
    if not config.TEXT_OUT.exists():
        print("(no artifacts/text yet)")
        return 0
    by_domain: Counter = Counter()
    by_ns: Counter = Counter()
    total = 0
    for path in sorted(config.TEXT_OUT.glob("*.jsonl")):
        for line in path.read_text(encoding="utf-8").splitlines():
            if not line.strip():
                continue
            rec = json.loads(line)
            by_domain[rec["domain"]] += 1
            by_ns[rec["namespace"]] += 1
            total += 1
    print(f"chunks: {total}")
    print("  by namespace:", dict(by_ns))
    print("  by domain:   ", dict(by_domain))
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="monomyth_corpus")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_fetch = sub.add_parser("fetch", help="fetch a source into artifacts/text")
    p_fetch.add_argument("source", choices=["gutenberg"])
    p_fetch.set_defaults(func=_cmd_fetch)

    sub.add_parser("verify", help="verify produced JSONL").set_defaults(func=_cmd_verify)
    sub.add_parser("stats", help="summarize produced chunks").set_defaults(func=_cmd_stats)
    sub.add_parser("qa", help="quality report over produced chunks").set_defaults(func=_cmd_qa)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
