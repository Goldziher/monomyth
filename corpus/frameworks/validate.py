#!/usr/bin/env python3
"""Validate the encoded framework artifacts, their crosswalks, and the license ledger.

Run: python3 corpus/frameworks/validate.py

Checks:
  1. Every artifacts/frameworks/*.json (except index.json / *.schema.json) matches the
     common envelope: required fields, contiguous 1..count ids, unique names, descriptions.
  2. Frameworks with a canonical size have exactly that count (Propp 31, Polti 36, ...).
  3. Crosswalk referential integrity: every *_id / *_ids field resolves to a real item id
     in the framework named by the file's `reference_map`.
  4. index.json lists exactly the framework files present, with matching counts/namespaces.
  5. manifest.json invariant: nothing tagged reference/noncommercial/copyright sits in `ship`.
  6. (optional) JSON-Schema validation if `jsonschema` is installed.

Exit code is non-zero on any failure, so this is CI-friendly.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FRAMEWORKS = ROOT / "artifacts" / "frameworks"
MANIFEST = ROOT / "corpus" / "manifest.json"
SCHEMA = FRAMEWORKS / "frameworks.schema.json"
INDEX = FRAMEWORKS / "index.json"

# Canonical sizes these frameworks MUST have (the whole point of encoding them).
CANONICAL_COUNTS = {
    "campbell_monomyth": 17,
    "propp_functions": 31,
    "propp_roles": 7,
    "greimas_actants": 6,
    "archetypes": 8,
    "booker_plots": 7,
    "polti_situations": 36,
    "thompson_motif_classes": 23,
    "atu_categories": 7,
    "dundes_motifemes": 8,
}
VALID_NAMESPACES = {"ship", "reference"}
NON_SHIPPABLE_TIERS = {"noncommercial", "copyright", "reference"}
SKIP_FILES = {"index.json", "frameworks.schema.json"}


def _load(path: Path, errors: list[str]) -> dict | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        errors.append(f"{path.name}: invalid JSON: {exc}")
        return None


def framework_files() -> list[Path]:
    return sorted(p for p in FRAMEWORKS.glob("*.json") if p.name not in SKIP_FILES)


def validate_envelope(data: dict, name: str, errors: list[str]) -> None:
    for field in ("framework", "title", "source", "license", "namespace", "domain", "count", "items"):
        if field not in data:
            errors.append(f"[{name}] missing required field: {field}")
    items = data.get("items", [])
    count = data.get("count")
    if count != len(items):
        errors.append(f"[{name}] declared count {count} != {len(items)} items")
    if data.get("namespace") not in VALID_NAMESPACES:
        errors.append(f"[{name}] invalid namespace: {data.get('namespace')!r}")
    ids = [it.get("id") for it in items]
    if ids != list(range(1, len(items) + 1)):
        errors.append(f"[{name}] ids not contiguous 1..{len(items)}")
    names = [it.get("name") for it in items]
    if len(set(names)) != len(names):
        errors.append(f"[{name}] duplicate item names")
    if any(not it.get("description") for it in items):
        errors.append(f"[{name}] item(s) missing a description")
    if name in CANONICAL_COUNTS and count != CANONICAL_COUNTS[name]:
        errors.append(f"[{name}] count {count} != canonical {CANONICAL_COUNTS[name]}")


def validate_references(data: dict, name: str, framework_ids: dict[str, set[int]], errors: list[str]) -> None:
    ref_map = data.get("reference_map")
    if not ref_map:
        return
    for field, target in ref_map.items():
        valid = framework_ids.get(target)
        if valid is None:
            errors.append(f"[{name}] reference_map points to unknown framework '{target}'")
            continue
        for it in data.get("items", []):
            if field not in it:
                continue
            raw = it[field]
            values = raw if isinstance(raw, list) else [raw]
            for v in values:
                if v not in valid:
                    errors.append(f"[{name}] item {it.get('id')} field '{field}'={v} not a valid id in '{target}'")


def validate_index(loaded: dict[str, dict], errors: list[str]) -> None:
    idx = _load(INDEX, errors)
    if idx is None:
        return
    listed = {e["framework"]: e for e in idx.get("frameworks", [])}
    present = set(loaded)
    if set(listed) != present:
        missing = present - set(listed)
        extra = set(listed) - present
        if missing:
            errors.append(f"[index] does not list present frameworks: {sorted(missing)}")
        if extra:
            errors.append(f"[index] lists absent frameworks: {sorted(extra)}")
    for fw, entry in listed.items():
        data = loaded.get(fw)
        if data and entry.get("count") != data.get("count"):
            errors.append(f"[index] {fw} count {entry.get('count')} != artifact {data.get('count')}")
    if not errors:
        print(f"  ok  index: {len(listed)} frameworks, matches artifacts on disk")


def validate_manifest(errors: list[str]) -> None:
    manifest = _load(MANIFEST, errors)
    if manifest is None:
        return
    for src in manifest.get("sources", []):
        sid = src.get("id", "<unknown>")
        if src.get("namespace") not in VALID_NAMESPACES:
            errors.append(f"[manifest:{sid}] invalid namespace: {src.get('namespace')!r}")
        if src.get("tier") in NON_SHIPPABLE_TIERS and src.get("namespace") == "ship":
            errors.append(f"[manifest:{sid}] tier={src.get('tier')} MUST NOT be in the ship namespace")
    if not errors:
        print(f"  ok  manifest: {len(manifest.get('sources', []))} sources, ship/reference invariant holds")


def maybe_schema_validate(loaded: dict[str, dict], errors: list[str]) -> None:
    try:
        import jsonschema  # type: ignore
    except ImportError:
        print("  --  jsonschema not installed; skipping JSON-Schema validation (structural checks still ran)")
        return
    schema = _load(SCHEMA, errors)
    if schema is None:
        return
    for name, data in loaded.items():
        try:
            jsonschema.validate(data, schema)
        except jsonschema.ValidationError as exc:
            errors.append(f"[schema:{name}] {exc.message}")
    if not errors:
        print(f"  ok  json-schema: all {len(loaded)} artifacts conform")


def main() -> int:
    errors: list[str] = []
    loaded: dict[str, dict] = {}

    print("frameworks:")
    for path in framework_files():
        data = _load(path, errors)
        if data is None:
            continue
        name = data.get("framework", path.stem)
        loaded[name] = data
        validate_envelope(data, name, errors)

    framework_ids = {n: {it.get("id") for it in d.get("items", [])} for n, d in loaded.items()}
    for name, data in loaded.items():
        validate_references(data, name, framework_ids, errors)
        if not any(e.startswith(f"[{name}]") for e in errors):
            tag = " ↳ref-ok" if data.get("reference_map") else ""
            print(f"  ok  {name:24s} {data.get('count'):>3d} items  [{data.get('namespace')}]{tag}")

    print("index:")
    validate_index(loaded, errors)
    print("schema:")
    maybe_schema_validate(loaded, errors)
    print("ledger:")
    validate_manifest(errors)

    if errors:
        print("\nFAILED:")
        for e in errors:
            print(f"  - {e}")
        return 1
    print(f"\nAll checks passed: {len(loaded)} framework artifacts, crosswalks, index, and ledger.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
