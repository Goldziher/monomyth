---
status: accepted
date: 2026-07-12
decision-makers: Na'aman Hirschfeld
---

# Reference retrieval quality: dedup, over-fetch, multi-query coverage (hybrid deferred)

## Context and Problem Statement

The reference-collection retrieval that feeds synthesis (ADR-0016) started as a naive single-query
vector KNN with no deduplication. In practice it returned **near-duplicate chunks** (the same passage
twice) and **clustered on one facet** of the source, so the grounding handed to the distillation
prompt was redundant and covered only part of the arc. The first synthesis run distilled its whole
law from six retrieved chunks, two of which were an identical literary aside and two an identical
intro — it never saw the trial, descent, or homecoming narratives at all. The lesson was that
retrieval *quality* was the problem, not that retrieval is the wrong tool: a system whose thesis is
RAG-grounded generation must do RAG well rather than dump whole documents into the prompt.

## Decision Drivers

- Synthesis exhaustiveness (ADR-0024) needs **distinct, arc-covering** grounding, not the *k* nearest
  neighbours to one abstract query.
- The **ship / surfaceable** retrieval path must stay byte-identical, because the content-fill cassette
  fixtures key on the exact grounding a query returns (ADR-0002 / the `monomyth-gen` replay test).
- We consume xberg as-is and do not fork it (ADR-0007).

## Considered Options

- **Whole-document grounding** — feed the ordered full source text into the prompt instead of
  retrieving. Bypasses RAG entirely and does not scale past a single myth.
- **Hybrid retrieval** — xberg's `RetrieveMode::Hybrid` (dense vector + FTS5 lexical, fused by RRF).
- **Vector + over-fetch + dedup + multi-query coverage** — keep dense retrieval but request more
  candidates than needed, drop near-duplicates, and union several queries for coverage.

## Decision Outcome

Chosen option: "vector + over-fetch + dedup + multi-query coverage". Concretely:

- **Dedup + over-fetch** (`monomyth-knowledge`): the reference path requests `top_k * REFERENCE_OVERFETCH`
  candidates and drops chunks whose whitespace/case-normalised text repeats a higher-ranked chunk,
  keeping the most relevant copy, before taking `top_k`. The ship path is left on plain vector,
  byte-identical, so the content-fill cassette stays valid.
- **Multi-query coverage** (`monomyth-synthesis::gather_grounding`): grounding is unioned across
  several queries — the seed query plus, mid-loop, the judge's named missing phases (ADR-0024) —
  deduplicated by `(source_id, normalized text)` keeping the highest score, sorted, and capped. This
  is RAG-native whole-arc coverage without dumping documents.

**Hybrid is deferred.** xberg's hybrid mode passes `query_text` straight to FTS5's `MATCH` parser,
which raises a syntax error on ordinary punctuation in a natural-language query (an apostrophe in
"hero's", a colon, a comma). Robust hybrid requires pre-embedding the raw query for the dense arm and
passing a *separately* FTS5-escaped term query for the lexical arm (xberg's hybrid does accept a
`query_vector` alongside `query_text`); until that lands, dense search plus dedup and multi-query
coverage is the safe path.

### Consequences

- Good, because dedup eliminated the duplicate-chunk pathology at its source, and multi-query + a
  broader seeded corpus recovered the two structural beats a single query missed: a re-run captured
  both the descent/nadir and the return/restoration that the first draft omitted.
- Good, because the surfaceable path is untouched — no fixture churn, no licensing-path change.
- Bad, because vector-only forgoes lexical exact-term matching until hybrid is made robust; tracked
  as a follow-up.
- Bad, because dedup is exact-normalised, not semantic — it catches identical/whitespace-different
  chunks, not paraphrase-level near-duplicates.

### Confirmation

`cargo test -p monomyth-knowledge` covers the dedup helper (unit) and a reference-retrieval round-trip
(integration); `cargo test -p monomyth-synthesis` covers `gather_grounding`'s cross-query dedup. The
qualitative confirmation is the v1→v2 comparison over the broader corpus: the same coarse prompt
produced a law that gained a distinct descent phase and a distinct return phase purely from better
retrieval.

## More Information

Supports ADR-0016 (grounding for the synthesize step) and ADR-0024 (the judge loop's targeted
re-retrieval rides this path). Builds on ADR-0007 (xberg) and ADR-0009 (local ONNX embeddings).
Follow-ups: robust hybrid retrieval (FTS5-escaped lexical arm + pre-embedded dense arm), and
framework-vocabulary query enrichment — seeding coverage queries from the taxonomy monomyth already
owns (Campbell stages, Propp functions), the monomyth-native analogue of grantflow's Wikidata
enrichment.
