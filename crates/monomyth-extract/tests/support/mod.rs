//! Shared fixtures for the `minimal_structure_extraction` cassette tests
//! (`minimal_structure_record.rs`, `minimal_structure_extraction.rs`).
//!
//! A `tests/support/mod.rs` is not itself compiled as a test binary (it has no
//! `#[test]`); each test file opts in with `mod support;`. Cargo compiles this
//! module as a fresh crate root per integration-test binary, so an item used by
//! one binary but not another is genuinely flagged `dead_code`/`unreachable_pub`
//! in the binary that doesn't use it — not because it is actually unused.
#![allow(
    dead_code,
    unreachable_pub,
    reason = "shared module compiled per test binary; see above"
)]

/// The `"provider/model"` routing label the cassette is recorded against and
/// stamped into the extracted [`Content`](monomyth_core::Content) provenance.
/// Only ever a label here — no real backend is called by either test file.
pub const MODEL: &str = "test/stub-model";

/// The fixed source passage both the recorder and the replay test extract
/// from. Short, thematically a "call to adventure", and stable so the two test
/// files build byte-identical prompts and therefore key-match the cassette.
pub const SOURCE_TEXT: &str = "A herald arrives at the quiet village and tells the young smith \
     that the old king has died without an heir, and only she carries the mark that can open the \
     mountain gate.";
