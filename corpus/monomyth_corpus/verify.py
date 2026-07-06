"""Verify produced JSONL: schema completeness + the ship/reference license invariant.

This is the enforcement counterpart to ledger.assert_shippable — it re-checks the
written artifacts (not just the intent), so a bad edit to a jsonl can't slip through.
"""

from __future__ import annotations

import json

from . import config, ledger

REQUIRED = (
    "id",
    "source",
    "work",
    "work_id",
    "license",
    "tier",
    "namespace",
    "domain",
    "checksum",
    "chunk_index",
    "text",
)


def verify_file(path) -> list[str]:
    errors: list[str] = []
    seen_ids: set[str] = set()
    n = 0
    for lineno, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        n += 1
        try:
            rec = json.loads(line)
        except json.JSONDecodeError as exc:
            errors.append(f"{path.name}:{lineno} invalid JSON: {exc}")
            continue
        for f in REQUIRED:
            if f not in rec or rec[f] in (None, ""):
                errors.append(f"{path.name}:{lineno} missing/empty field: {f}")
        cid = rec.get("id")
        if cid in seen_ids:
            errors.append(f"{path.name}:{lineno} duplicate chunk id: {cid}")
        seen_ids.add(cid)

        ns = rec.get("namespace")
        if ns not in config.VALID_NAMESPACES:
            errors.append(f"{path.name}:{lineno} invalid namespace: {ns!r}")
        # The core invariant, re-checked against the ledger by source id.
        src = rec.get("source")
        try:
            declared = ledger.get(src)
            if rec.get("namespace") != declared.get("namespace"):
                errors.append(f"{path.name}:{lineno} namespace {ns} != ledger {declared.get('namespace')} for '{src}'")
            if ns == "ship" and declared.get("tier") in config.NON_SHIPPABLE_TIERS:
                errors.append(f"{path.name}:{lineno} non-shippable tier in ship output for '{src}'")
        except ledger.LicenseError as exc:
            errors.append(f"{path.name}:{lineno} {exc}")
    return errors + ([] if n else [f"{path.name}: empty file"])


def verify_all() -> tuple[int, list[str]]:
    files = sorted(config.TEXT_OUT.glob("*.jsonl")) if config.TEXT_OUT.exists() else []
    all_errors: list[str] = []
    for path in files:
        errs = verify_file(path)
        status = "FAIL" if errs else "ok"
        count = sum(1 for ln in path.read_text(encoding="utf-8").splitlines() if ln.strip())
        print(f"  {status:4s} {path.name:24s} {count:5d} chunks")
        all_errors += errs
    return len(files), all_errors
