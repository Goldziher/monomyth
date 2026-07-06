"""Tiny stdlib HTTP GET with a polite UA, retries, and an on-disk raw cache.

Caching makes the pipeline idempotent and re-runnable offline: once a URL is
fetched, its bytes live under corpus/raw/ keyed by a hash of the URL.
"""

from __future__ import annotations

import gzip
import hashlib
import os
import ssl
import time
import urllib.error
import urllib.request

from . import config

# System CA bundles to fall back to when Python's default trust store is empty
# (some interpreters — e.g. a freshly built 3.14 — ship without configured certs).
_CA_FALLBACKS = ("/etc/ssl/cert.pem", "/etc/ssl/certs/ca-certificates.crt")


def _ssl_context() -> ssl.SSLContext:
    ctx = ssl.create_default_context()
    if ctx.cert_store_stats().get("x509", 0) == 0:
        for path in (os.environ.get("SSL_CERT_FILE"), *_CA_FALLBACKS):
            if path and os.path.exists(path):
                ctx.load_verify_locations(path)
                break
    return ctx


_SSL = _ssl_context()


def _cache_path(url: str):
    key = hashlib.sha256(url.encode("utf-8")).hexdigest()[:20]
    return config.RAW / f"{key}.bin"


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def get(url: str, *, retries: int = 3, timeout: int = 30, use_cache: bool = True) -> bytes:
    """GET url as bytes, transparently gunzipping and caching to corpus/raw/."""
    cache = _cache_path(url)
    if use_cache and cache.exists():
        return cache.read_bytes()

    config.RAW.mkdir(parents=True, exist_ok=True)
    last_err: Exception | None = None
    for attempt in range(1, retries + 1):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": config.USER_AGENT, "Accept-Encoding": "gzip"})
            with urllib.request.urlopen(req, timeout=timeout, context=_SSL) as resp:
                raw = resp.read()
                if resp.headers.get("Content-Encoding") == "gzip":
                    raw = gzip.decompress(raw)
            if use_cache:
                cache.write_bytes(raw)
            return raw
        except (urllib.error.URLError, urllib.error.HTTPError, TimeoutError) as exc:
            last_err = exc
            if attempt < retries:
                time.sleep(1.5 * attempt)
    raise RuntimeError(f"GET failed after {retries} attempts: {url} ({last_err})")


def get_json(url: str, **kw) -> dict:
    import json

    return json.loads(get(url, **kw).decode("utf-8"))


def get_text(url: str, **kw) -> tuple[str, bytes]:
    """Return (decoded_text, raw_bytes) — raw bytes kept for checksumming."""
    raw = get(url, **kw)
    for enc in ("utf-8", "latin-1"):
        try:
            return raw.decode(enc), raw
        except UnicodeDecodeError:
            continue
    return raw.decode("utf-8", errors="replace"), raw
