"""Repo paths and pipeline constants — single source of truth for locations."""

from __future__ import annotations

from pathlib import Path

# corpus/monomyth_corpus/config.py -> repo root is three parents up.
ROOT = Path(__file__).resolve().parents[2]

CORPUS = ROOT / "corpus"
MANIFEST = CORPUS / "manifest.json"
SOURCES = CORPUS / "sources"
RAW = CORPUS / "raw"  # cached raw downloads (gitignored)

ARTIFACTS = ROOT / "artifacts"
TEXT_OUT = ARTIFACTS / "text"  # chunked, tagged JSONL output
GRAPH_OUT = ARTIFACTS / "graph"  # RDF/TTL output (Wikidata etc.)

USER_AGENT = "monomyth-corpus/0.1 (+https://github.com/monomyth; research/indexing)"

# Chunking: greedy paragraph packing up to CHUNK_MAX_CHARS; ~4 chars/token heuristic.
CHUNK_MAX_CHARS = 1400
CHARS_PER_TOKEN = 4

VALID_NAMESPACES = {"ship", "reference"}
NON_SHIPPABLE_TIERS = {"noncommercial", "copyright", "reference"}
