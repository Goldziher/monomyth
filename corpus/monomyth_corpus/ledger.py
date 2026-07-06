"""Load the license ledger (manifest.json) and resolve/enforce source licensing.

This is the single gatekeeper for the ship/reference invariant: every chunk the
pipeline writes must carry the namespace its source declares, and no source tagged
reference/noncommercial/copyright may ever land in the `ship` namespace.
"""

from __future__ import annotations

import json
from functools import lru_cache

from . import config


class LicenseError(RuntimeError):
    """Raised when an ingest would violate the ship/reference invariant."""


@lru_cache(maxsize=1)
def _manifest() -> dict:
    return json.loads(config.MANIFEST.read_text(encoding="utf-8"))


@lru_cache(maxsize=1)
def _sources() -> dict[str, dict]:
    return {s["id"]: s for s in _manifest().get("sources", [])}


def get(source_id: str) -> dict:
    """Return the ledger entry for a source id, or raise if it is not declared."""
    src = _sources().get(source_id)
    if src is None:
        raise LicenseError(
            f"source '{source_id}' is not declared in the manifest ledger; "
            f"add it to corpus/manifest.json before fetching."
        )
    return src


def tags(source_id: str) -> dict:
    """The tags every chunk from this source must carry."""
    src = get(source_id)
    return {
        "source": source_id,
        "license": src.get("license", "UNKNOWN"),
        "tier": src.get("tier", "UNKNOWN"),
        "namespace": src.get("namespace", "reference"),
        "domain": src.get("domain", "unknown"),
        "url": src.get("url"),
    }


def assert_shippable(source_id: str) -> None:
    """Guard used before writing into a ship output: refuse non-shippable sources."""
    src = get(source_id)
    ns = src.get("namespace")
    tier = src.get("tier")
    if ns not in config.VALID_NAMESPACES:
        raise LicenseError(f"[{source_id}] invalid namespace: {ns!r}")
    if ns == "ship" and tier in config.NON_SHIPPABLE_TIERS:
        raise LicenseError(
            f"[{source_id}] tier={tier} may not be in the ship namespace — "
            f"fix the ledger or route this source to `reference`."
        )
    if ns != "ship":
        raise LicenseError(
            f"[{source_id}] is namespace={ns!r}; refusing to write it into a ship output. "
            f"Reference-tier sources inform generation but are never surfaced verbatim."
        )
