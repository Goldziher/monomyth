---
status: "accepted"
date: 2026-07-29
decision-makers: Na'aman Hirschfeld
---

# MIT license and experimental status

## Context and Problem Statement

monomyth was initially framed as a commercial, source-available product, released under the Business
Source License 1.1. [ADR-0005](./0005-commercial-licensing-ship-reference.md) motivated the
`ship`/`reference` corpus invariant by appeal to that commercial status ("monomyth is a commercial
product, so …"), and [ADR-0019](./0019-user-uploaded-media-namespace.md) /
[ADR-0026](./0026-reference-path-first-rag-enrichment.md) restate that framing.

The project is now an open, **experimental** engine. This ADR records two changes: the code is
relicensed to **MIT**, and the project's posture is **experimental / pre-1.0**. It also re-motivates
the corpus invariant so it no longer rests on a "commercial product" premise — because that premise
is gone, but the invariant must not weaken.

## Decision Drivers

- Openness and adoption — a permissive, universally understood license lowers the barrier to use.
- The `ship`/`reference` corpus invariant is about the copyright of *third-party corpus text*, and is
  independent of monomyth's own code license. It must stay valid and strictly enforced either way.
- Honest maturity signaling — the API and the serialized schema are unstable.

## Considered Options

- Keep BUSL-1.1 (source-available, non-production without a commercial license).
- MIT (permissive).
- Apache-2.0 (permissive, patent grant).

## Decision Outcome

Chosen option: **MIT**, with the project marked **experimental / pre-1.0**. MIT is the simplest,
most widely understood permissive license and imposes no obligations on downstream users, which suits
an experimental engine meant to be read, forked, and built on.

Crucially, MIT-licensed output may be used for **any** purpose, **including commercially, by anyone**.
That makes the reference-only invariant *more* important, not less: copyrighted and NonCommercial
sources must never be surfaced verbatim, precisely because downstream commercial use is now
unrestricted. The invariant of ADR-0005 therefore stands unchanged and stays strictly enforced; only
its *justification* is re-based from "we are a commercial product" to "our permissively-licensed
output may be used commercially."

### Consequences

- Good — permissive, familiar, friction-free adoption; the corpus invariant gains a stronger, more
  general rationale that does not depend on a business model.
- Good — the experimental label sets correct expectations while the contract stabilizes.
- Bad — MIT provides no commercial protection for the author; anyone may build a competing product on
  the engine. Accepted deliberately.
- Neutral — the corpus `commercial: true` policy flag is retained as the internal name for
  strict-enforcement mode (permissive output ⇒ treat corpus handling as commercial-grade).

### Confirmation

- `LICENSE` contains the MIT text; `Cargo.toml` `[workspace.package] license = "MIT"`; `cargo deny
  check` passes (MIT is already in the allow list).
- The corpus enforcement is untouched: `monomyth-knowledge`'s ledger/audit tests, including
  `embedded_manifest_declares_commercial_use` (which requires `manifest.commercial == true`), remain
  green. The re-motivation is documentation-only — no enforcement path changed.

## More Information

Per this repository's convention, ADRs are immutable once accepted; a changed decision is a new ADR
that supersedes or re-motivates the old one, never an in-place edit. Accordingly the bodies of
ADR-0005 / ADR-0019 / ADR-0026 are left as written and should be read through this ADR: wherever they
say "commercial product," read "permissively-licensed output that may be used commercially." The
`adrs/README.md` index annotates ADR-0005's status to point here.
