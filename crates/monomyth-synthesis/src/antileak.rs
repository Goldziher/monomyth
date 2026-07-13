//! The machine-checked anti-leak gate (ADR-0016's 4th enforcement layer).
//!
//! ADR-0005 already enforces the ship/reference licensing invariant at ingest,
//! at retrieval, and in CI. Build-time synthesis adds a 4th layer specific to
//! LLM distillation: even though the model is only ever asked to produce an
//! abstract taxonomy (never asked to quote), a model can still reproduce
//! reference-passage wording by accident or by over-fitting to its prompt
//! context. [`verify_no_verbatim`] is the machine-checked backstop that catches
//! that failure mode before a candidate ever reaches a pre-review artifact.
//!
//! # Honest limitation
//!
//! This is a **verbatim-span detector**, not a plagiarism or semantic-copying
//! detector. It catches an exact (case/punctuation-insensitive) shared run of
//! [`SHINGLE_N`] or more words; it cannot catch a close paraphrase that
//! reproduces a reference passage's structure and word choice without sharing
//! an 8-word run. The real ceiling on copying is upstream of this gate:
//! [`monomyth_frameworks::Tier`]'s "system" tier (an uncopyrightable idea, not
//! prose) plus mandatory human review before a law can ever load
//! (`monomyth_frameworks::load_law`'s `MissingReviewer` check). This gate is a
//! cheap, deterministic tripwire for the most common and most damaging failure
//! (verbatim reproduction), not a substitute for that review.

use std::collections::BTreeSet;

use crate::error::SynthesisError;

/// Shingle width, in words. An 8-gram is long enough that a coincidental
/// shared short phrase ("the hero returns", "a mediator stands between") does
/// not trip the gate, while still catching a meaningfully-sized verbatim span.
const SHINGLE_N: usize = 8;

/// FNV-1a 64-bit offset basis, per the reference algorithm. Matches the
/// constant used in `monomyth_llm::cassette` for the same algorithm, so the
/// choice of hash is consistent across the workspace.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64-bit prime, per the reference algorithm.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a over `bytes`, dependency-free and stable across platforms.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// Normalize `text` into lowercase words with ASCII punctuation stripped and
/// whitespace collapsed, so `"The Suppliant implores!"` and `"the suppliant
/// implores"` shingle identically.
///
/// Shared with [`crate::prescore`] so the deterministic pre-score tokenizes
/// candidate and grounding text exactly as the anti-leak gate does.
pub(crate) fn normalize_words(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(|word| {
            word.chars()
                .filter(|character| !character.is_ascii_punctuation())
                .flat_map(char::to_lowercase)
                .collect::<String>()
        })
        .filter(|word| !word.is_empty())
        .collect()
}

/// The word-level 8-gram shingle hashes of `text`, keyed by hash with the
/// reconstructed span text as the value (so a match can report a human-legible
/// offending span). Text with fewer than [`SHINGLE_N`] words produces no
/// shingles — a short shared phrase therefore cannot trip the gate, which is
/// correct: the gate targets meaningfully-sized verbatim reproduction, not
/// incidental short overlaps.
fn shingles(text: &str) -> Vec<(u64, String)> {
    let words = normalize_words(text);
    if words.len() < SHINGLE_N {
        return Vec::new();
    }
    words
        .windows(SHINGLE_N)
        .map(|window| {
            let span = window.join(" ");
            (fnv1a(span.as_bytes()), span)
        })
        .collect()
}

/// Verify that none of `candidate_texts` verbatim-overlaps (as an 8-word
/// shingle) any chunk in `reference_chunks`.
///
/// Each reference chunk is `(source_id, text)`. Every candidate text is
/// checked against every reference chunk; the first match found is reported.
///
/// # Errors
///
/// Returns [`SynthesisError::VerbatimOverlap`] naming the offending reference
/// source id and a reconstructed 8-word span from the candidate text that
/// matches it.
pub fn verify_no_verbatim(
    candidate_texts: &[&str],
    reference_chunks: &[(&str, &str)],
) -> Result<(), SynthesisError> {
    let reference_shingles: Vec<(&str, BTreeSet<u64>)> = reference_chunks
        .iter()
        .map(|(source_id, text)| {
            let hashes = shingles(text).into_iter().map(|(hash, _)| hash).collect();
            (*source_id, hashes)
        })
        .collect();

    for candidate_text in candidate_texts {
        for (hash, span) in shingles(candidate_text) {
            if let Some((source_id, _)) = reference_shingles
                .iter()
                .find(|(_, hashes)| hashes.contains(&hash))
            {
                return Err(SynthesisError::VerbatimOverlap {
                    candidate_span: span,
                    source_id: (*source_id).to_owned(),
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::verify_no_verbatim;
    use crate::error::SynthesisError;

    /// A reference chunk long enough to contain an unambiguous 8-word run.
    const REFERENCE_TEXT: &str =
        "The Suppliant implores a Power in authority to grant a boon of mercy.";

    #[test]
    fn rejects_a_candidate_embedding_a_verbatim_eight_word_span() {
        let candidate =
            "In this taxonomy, the suppliant implores a power in authority to describe a stage.";

        let error = verify_no_verbatim(&[candidate], &[("perseus", REFERENCE_TEXT)])
            .expect_err("an embedded 8-word verbatim run must trip the gate");

        match error {
            SynthesisError::VerbatimOverlap {
                source_id,
                candidate_span,
            } => {
                assert_eq!(source_id, "perseus", "must name the offending source");
                assert_eq!(
                    candidate_span, "the suppliant implores a power in authority to",
                    "must reconstruct the exact offending normalized span"
                );
            }
            other => panic!("expected VerbatimOverlap, got {other:?}"),
        }
    }

    #[test]
    fn accepts_a_genuine_paraphrase_of_the_same_idea() {
        let candidate =
            "A dependent figure petitions a higher authority, seeking aid it cannot compel.";

        verify_no_verbatim(&[candidate], &[("perseus", REFERENCE_TEXT)])
            .expect("a genuine paraphrase sharing no 8-word run must pass");
    }

    #[test]
    fn accepts_a_short_shared_phrase_under_the_shingle_width() {
        // Shares "a power in authority" (four words) with REFERENCE_TEXT, well ~keep
        // under SHINGLE_N — too short and common a phrase to be meaningful ~keep
        // evidence of copying. ~keep
        let candidate = "A power in authority governs the realm in this taxonomy's second stage.";

        verify_no_verbatim(&[candidate], &[("perseus", REFERENCE_TEXT)])
            .expect("a shared phrase shorter than the shingle width must pass");
    }

    #[test]
    fn accepts_empty_inputs() {
        verify_no_verbatim(&[], &[]).expect("no candidate texts and no reference chunks must pass");
        verify_no_verbatim(&["short"], &[])
            .expect("a candidate text with no reference chunks to compare against must pass");
        verify_no_verbatim(&[], &[("perseus", REFERENCE_TEXT)])
            .expect("no candidate texts must pass regardless of reference chunks");
    }
}
