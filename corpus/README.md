# monomyth corpus pipeline

Milestone 0 of monomyth: build the comparative-mythology domain corpus + RAG foundation
that grounds the engine's domain model and (later) feeds runtime retrieval.

See the full plan: `~/.claude/plans/we-are-building-an-flickering-puppy.md`.

## Two jobs

1. **Distill** the domain schema (stages, functions, motifs, roles, entity types) from
   established narrative/folklore frameworks + corpus statistics → becomes the Rust model's
   enums (`crates/monomyth-core/src/story/`) and the `ContentPrompt` taxonomy.
2. **Retrieve** at runtime — the hybrid generator (`monomyth-gen`) queries a knowledge graph
   (entities/relations) + a vector index (exemplar passages) to ground structure and prose.

## Licensing model (this is a COMMERCIAL product)

Every artifact row/triple/chunk carries `license` + `namespace` + `domain` tags.

- **`ship`** namespace — PD / CC0 / CC-BY / CC-BY-SA only. May be surfaced verbatim in output.
  CC-BY-SA content is *isolated* so its share-alike obligation can't contaminate PD/CC0 material.
- **`reference`** namespace — copyrighted or NonCommercial (Dúchas, Perseus, TV Tropes, in-copyright
  canon). Informs generation (structure/priors) but is **never redistributed / surfaced verbatim**.

The `manifest.json` is the per-source license + namespace ledger — the provenance ground truth.
An automated check asserts no `reference`-tagged content ever lands in the `ship` namespace.

## Layout

```text
corpus/
├── manifest.json        # license + namespace ledger (grows as sources are added)
├── frameworks/          # encode + validate the analytic frameworks (Propp/Polti/Campbell/…)
│   └── validate.py      # envelope + canonical counts + crosswalk ref-integrity + index + ledger
│                        #   optional: `pip install jsonschema` enables JSON-Schema validation too
├── select/              # (later) Gutendex/SPGC genre selection, Wikidata subsetting
├── fetch/               # (later) PG harvest, HF datasets, SPARQL, Wikisource API, Fandom dumps
├── normalize/           # (later) boilerplate strip, wikitext→text, OCR, chunk, entity-link
└── distill/             # (later) corpus stats → priors; Claude reconciliation → schema

../artifacts/            # generated outputs consumed by the Rust engine
└── frameworks/
    ├── frameworks.schema.json   # JSON Schema for the common envelope
    ├── index.json               # tiered index of all framework artifacts
    ├── campbell_monomyth.json / booker_plots.json / atu_categories.json   # MACRO: arc + genre
    ├── propp_functions.json / polti_situations.json / dundes_motifemes.json # MESO: beat grammar
    ├── thompson_motif_classes.json                                        # MICRO: content-atom roots
    ├── propp_roles.json / greimas_actants.json / archetypes.json          # CHARACTER facets
    └── character_crosswalk.json / arc_crosswalk.json / plot_crosswalk.json # CROSSWALKS (bind the tiers)
```

## The schema, not just a list of lists

The frameworks compose into a **3-tier plot model + a character model**, wired by three crosswalks:

- **macro → meso**: `arc_crosswalk` maps each Campbell stage to the Propp functions that realize it.
- **genre → conflict**: `plot_crosswalk` maps each Booker plot to representative Polti situations.
- **character**: `character_crosswalk` aligns Propp role × Greimas actant × archetype per dramatic role.

Crosswalks carry a `reference_map` and the validator enforces that every referenced id exists —
so the tiers can never drift out of sync. This is the schema seed the Rust `story/` module reads.

## Framework artifact schema

Each `artifacts/frameworks/*.json` file:

```jsonc
{
  "framework": "propp_functions",
  "title": "Propp — Functions of Dramatis Personae",
  "source": "Vladimir Propp, Morphology of the Folktale (1928)",
  "license": "system/idea (not copyrightable); descriptions authored for monomyth",
  "namespace": "ship",
  "domain": "framework",
  "tier_note": "The 31-function *system* is freely modelable; we do not reproduce Propp's prose.",
  "count": 31,
  "items": [ /* { id, symbol?, name, act?/axis?/section?, description, roles? } */ ]
}
```

## Fetch pipeline (steps 2-3) — `monomyth_corpus`

Stdlib-only (no third-party deps). Staged, idempotent, deterministic, license-enforced.

```text
python -m monomyth_corpus fetch gutenberg   # topic-driven PD pull -> artifacts/text/<domain>.jsonl + catalog.json
python -m monomyth_corpus verify            # re-check schema + ship/reference invariant on all jsonl
python -m monomyth_corpus qa                # quality report (token dist, boilerplate, mojibake, dups)
python -m monomyth_corpus stats             # chunk counts by namespace / domain
```

Flow: **select** (`corpus/sources/gutenberg_topics.json` — per-domain Gutendex topics; `copyright=false`
is PG's US-PD determination and thus our ship gate) → **fetch** (cached to `corpus/raw/`, gitignored;
individual failures are warned + skipped) → **normalize** (strip PG boilerplate + credits, unwrap
paragraphs) → **chunk** (deterministic, `~1400` chars) → **tagged JSONL, organized per domain** →
**verify** + **qa**. Every chunk carries `license`/`tier`/`namespace`/`domain` + provenance
(`url`, `retrieved`, `checksum`); `ledger.assert_shippable` refuses to write a `reference` source into a
`ship` output, and `verify` re-asserts that against the written files.

Outputs (all under `artifacts/text/`, regenerated not committed): `<domain>.jsonl` (myth, folklore,
esoterica, newage, scifi, fantasy), `catalog.json` (one row per fetched work), `qa_report.json`.
Designed-next module: `fetch/wikidata.py` (SPARQL subset → `artifacts/graph/*.ttl` → Oxigraph).
Deferred (needs heavy deps / Rust side): HF-parquet bulk dumps, embedding + LanceDB indexing.

## Build order (step 1 = this)

1. **Framework encoding** (here) — Polti-36, Propp 31+7, Campbell 17, Greimas 6, archetypes,
   Booker 7. ATU/Thompson tables come from the `trilogy` (CC-BY-SA) dataset in a later fetch step.
2. Ship-safe text baseline → LanceDB `ship`.
3. Knowledge graph (Wikidata subset → Oxigraph).
4. Priors + distillation.
5. Reference namespace.
6. Wire retrieval into Rust.
