---
status: accepted
date: 2026-07-13
decision-makers: Na'aman Hirschfeld
---

# Reference-path-first RAG enrichment: keywords and entities as generation priors

## Context and Problem Statement

xberg's `DocumentRecord` carries `keywords` and `entities` slots on every ingested document, and
monomyth had left both empty since ADR-0007. Populating them can sharpen the priors handed to
synthesis and reference retrieval (ADR-0016, ADR-0025) — but monomyth is a commercial product with a
hard invariant (ADR-0005): `reference`-namespace material informs generation and is never surfaced
verbatim, while `ship` material may be shown to a user. The **ship** retrieval path is also pinned by
recorded content-fill cassettes and determinism goldens (ADR-0002), so any enrichment must not perturb
it — not its passage list, not its order, not the bytes of the built prompt.

## Decision Drivers

- Reference retrieval quality (ADR-0025) still leaves lexically-unrelated candidates in the
  over-fetched pool; keyword/entity overlap is a cheap additional signal to bias priors toward
  on-topic grounding.
- The ship path's cassette fixtures and determinism goldens must stay byte-identical — enrichment
  work must be provably inert on that path, not just "probably fine."
- monomyth consumes xberg as-is and does not fork it (ADR-0007); enrichment reuses xberg's existing
  keyword/entity slots rather than inventing a parallel metadata channel.
- Default behavior for every existing consumer (including cassette-bearing crates) must be unchanged
  until a caller explicitly opts in.

## Considered Options

- **Ship-path-first enrichment** — populate and filter on keywords/entities for surfaceable retrieval
  too, hoping the cassette fixtures can be re-recorded.
- **No enrichment** — leave `keywords`/`entities` empty indefinitely; keep relying on dense (and now
  hybrid) vector retrieval alone.
- **Reference-path-first enrichment** — populate both slots at ingest, best-effort, and use them only
  to bias the reference (priors-only) retrieval path, leaving the ship path untouched.

## Decision Outcome

Chosen option: "reference-path-first enrichment", because it gets the quality benefit where it is
needed (reference grounding for synthesis) without putting the cassette/golden-pinned ship path at
risk. Concretely, delivered in slices:

- **Slice 1 — keywords at ingest** (shipped, commit `158a857`): pure YAKE/RAKE keyword extraction (no
  model), gated behind a default-OFF `rag-keywords` Cargo feature *and* a runtime opt-in,
  `Knowledge::with_keyword_enrichment(KeywordEnrichment)`. `KeywordEnrichment` defaults to disabled.
  Extraction (`extract_keywords_best_effort`) is **never fatal to an ingest**: a failure is logged and
  treated as "no keywords," and the disabled/feature-off case returns empty without attempting
  extraction at all.
- **Slice 2 — reference-path keyword filter** (shipped, commit `bccb381`): `reference_keyword_filter`
  turns a query's salient terms into a disjunction of `Filter::ArrayContains` predicates over
  `doc.keywords` (the `DOC_KEYWORDS_FIELD` constant), capped at `MAX_QUERY_KEYWORD_PREDICATES = 8` to
  bound per-candidate evaluation and stay within the filter IR's complexity cap. `retrieve_reference`
  tries the filtered fetch first and **degrades to the unfiltered path** whenever no reference document
  shares a keyword with the query, so the distiller is never starved of grounding it would otherwise
  have received. With enrichment disabled (the default), the filter is `None` and reference retrieval
  is byte-for-byte the prior unfiltered behavior.
- **Slice 3 — entities (NER) at ingest** (shipped): best-effort named-entity extraction at ingest,
  mirroring the keyword slice's shape — never fatal, default-OFF Cargo feature plus a runtime opt-in
  (`EntityEnrichment`, analogous to `KeywordEnrichment`; `Knowledge::with_entity_enrichment`).
  `extract_entities_best_effort` calls `xberg::detect_entities` and stores the result on `doc.entities`
  as a **deduped, sorted JSON array of plain entity surface-strings** — the same shape as `doc.keywords`,
  so a `Filter::ArrayContains` predicate over `doc.entities` addresses it identically. The slot is left
  at its `Value::Null` default unless extraction actually yields entities, so a disabled/empty run
  writes exactly the bytes it wrote before. Two backend variants, both feature-gated and off by default:
  - `rag-ner-llm` — an LLM-backed extractor (`EntityEnrichment::llm`, or `with_backend` for an injected
    fake in tests). Recommended default backend: it only ever writes reference-path metadata, so its
    inherent nondeterminism never reaches a ship prompt or a determinism-pinned cassette. Entities target
    mythology-relevant custom categories — `Deity`, `Hero`, `Artifact`, `Realm` — passed as zero-shot
    labels rather than a fixed closed set.
  - `rag-ner-onnx` — a local ONNX (GLiNER-style) extractor for zero-shot entity typing without an LLM
    call. Available but off by default, since it pulls in `ort`/`hf-hub` and a model download that most
    consumers do not need.

  **Deliberately deferred from slice 3** (a future slice, not this decision): the reference-path *entity*
  filter (the entity analogue of slice 2's keyword filter) and any structured `entities_detail` storage
  (category/offsets/confidence). Slice 3 proves the population + `ArrayContains`-addressability end to
  end; a query-side entity filter would cost a live LLM call per retrieval (unlike cheap YAKE keywords),
  which is a distinct cost/design trade-off warranting its own slice.

### The hard invariant: the ship path never reads either slot

The ship (surfaceable) path — `retrieve_surfaceable`, which issues `RetrieveMode::Vector` with
`Filter::Eq{doc.metadata.namespace="ship"}` (`ship_filter`) — never references `doc.keywords` or
`doc.entities` in its query, its filter, or its ranking. Populating those columns at ingest changes
bytes the ship filter never reads; it cannot change which chunks match, their score, or their order.
The surfaceable passage list is therefore byte-identical whether or not enrichment ran at ingest, so
the prompt built from it and every cassette key derived from it are unaffected.

This is enforced structurally, not just by convention:

- Both enrichment features are default-OFF **and** require a separate runtime opt-in
  (`with_keyword_enrichment` / `with_entity_enrichment`) — two independent gates, either of which
  alone is sufficient to keep default behavior unchanged.
- No cassette-bearing crate opts in to either feature or setter.
- A byte-identity guard test per slot ingests the identical ship corpus into two stores — one enriched,
  one not — runs the identical surfaceable query against both, and asserts the returned passages
  serialize to byte-identical JSON (`keyword_enrichment_does_not_perturb_surfaceable_retrieval` and
  `entity_enrichment_does_not_perturb_surfaceable_retrieval`). A companion populate-when-enabled test
  per slot proves enrichment is not a silent no-op, so the byte-identity test cannot pass vacuously
  because extraction never ran.

**Forbidden, and would constitute drift from this decision:** any keyword or entity predicate, filter,
or re-rank applied on the ship path. If future work wants keyword/entity-aware surfaceable retrieval,
that is a new ADR with its own cassette-migration plan — not a silent extension of this one.

### Consequences

- Good, because reference retrieval (feeding synthesis, ADR-0016/ADR-0025) gains a cheap lexical/entity
  overlap signal without any risk to the cassette- and golden-pinned ship path — the two gates
  (feature flag + runtime opt-in) and the byte-identity tests make that a checked property, not an
  assumption.
- Good, because best-effort extraction (never fatal to ingest) means a broken or slow extractor
  degrades to "no keywords/entities," not a failed ingest.
- Good, because the `rag-ner-llm` backend's nondeterminism is quarantined to reference metadata by
  construction — it cannot reach a ship prompt or a cassette, so we get LLM-quality entity typing
  without reopening the determinism argument (ADR-0002) that governs the procedural/content split.
- Bad, because the degrade-to-unfiltered fallback (slice 2) trades some retrieval precision for
  recall-safety: a filter that would have usefully narrowed the pool is discarded outright rather than
  partially relaxed when it returns zero results.
- Bad, because slice 3 ships the entity *population* but not yet the reference-path entity *filter* or
  structured `entities_detail`, so the stored entities are addressable but not yet used to narrow
  reference retrieval — deferred value, tracked as follow-up.
- Bad, because `EntityEnrichment` (like `KeywordEnrichment` before it) lands as a typed `Knowledge`
  field — an interim config home, not `monomyth.toml`, pending the `[knowledge.*]` section (WS-A,
  ADR-0015) in a later slice.
- Neutral, because `rag-ner-onnx` exists for a future zero-LLM-dependency path but is not the
  recommended default; it is a documented alternative, not a competing decision.

### Confirmation

`cargo test -p monomyth-knowledge --features rag-keywords` covers the keyword slices: the byte-identity
guard, the populate-when-enabled guard, the filter-shape test
(`reference_keyword_filter_builds_array_contains_disjunction_over_doc_keywords`), the
disabled-by-default test (`reference_keyword_filter_is_none_when_enrichment_disabled`, unconditional —
it holds regardless of the `rag-keywords` feature), and the degrade-to-unfiltered test
(`reference_retrieval_degrades_to_unfiltered_when_no_document_shares_a_keyword`).
`cargo test -p monomyth-knowledge --features rag-ner-llm` covers slice 3: the entity byte-identity guard
(`entity_enrichment_does_not_perturb_surfaceable_retrieval`), the populate-when-enabled guard
(`entity_enrichment_populates_entities_when_enabled`), and the end-to-end addressability proof
(`reference_document_is_retrievable_by_a_detected_entity_via_array_contains`). `cargo test --workspace`
(no enrichment features enabled) is the default-behavior check: every cassette and both determinism
goldens (gen `0x65f6_6a4b_a541_5419`, eval `0xe852_73b6_ce90_fe55`) must still pass unmodified.

## Pros and Cons of the Options

### Ship-path-first enrichment

- Bad, because it puts the cassette- and golden-pinned surfaceable path at risk for a benefit (sharper
  reference priors) that does not require touching it.

### No enrichment

- Bad, because it leaves reference retrieval without a lexical/entity-overlap signal that ADR-0025's
  own dedup/over-fetch work already showed is worth having (near-duplicate and off-topic candidates
  both survive plain vector search).

### Reference-path-first enrichment

- Good, because the benefit (better reference grounding) and the risk (ship-path perturbation) are
  structurally separated — the ship path cannot see either metadata slot.
- Neutral, because it requires shipping two new best-effort extraction stages (keywords, entities) and
  their opt-in plumbing, rather than a single change.

## More Information

Builds on ADR-0005 (the ship/reference licensing invariant this ADR is careful never to blur), ADR-0007
(consume xberg as-is — both slots and both NER backends are xberg surface, not a fork), ADR-0016
(reference ingest and the priors-only synthesis pipeline this enrichment feeds), and ADR-0025 (the
reference retrieval quality work — dedup, over-fetch, hybrid — that this ADR adds a further narrowing
signal on top of). The permanent config home for `KeywordEnrichment`/`EntityEnrichment` is tracked
against ADR-0015's `[knowledge.*]` TOML section. Deferred follow-ups: the reference-path entity filter,
structured `entities_detail` storage, and config wiring of the enrichment knobs. Revisit if a future
need for keyword/entity-aware *surfaceable* retrieval arises — that requires a new cassette-migration
plan and its own ADR.
