//! A tiny, dependency-free FNV-1a hash over raw bytes.
//!
//! Mirrors `monomyth-gen/tests/determinism.rs`'s helper: this crate pins fixture
//! content and `Report` snapshots the same way the generator pins its golden
//! seed-42 output, so both use the same cheap, dependency-free hash rather than
//! pulling in a hashing crate for a one-off checksum.

/// FNV-1a 64-bit offset basis.
const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64-bit prime.
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Hash `bytes` with 64-bit FNV-1a.
///
/// ```
/// use monomyth_eval::util::fnv1a;
///
/// assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
/// ```
#[must_use]
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = FNV_OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnv1a_of_empty_input_should_equal_the_offset_basis() {
        assert_eq!(fnv1a(b""), FNV_OFFSET_BASIS);
    }

    #[test]
    fn fnv1a_should_be_deterministic_across_calls() {
        let bytes = b"monomyth-eval";
        assert_eq!(fnv1a(bytes), fnv1a(bytes));
    }

    #[test]
    fn fnv1a_should_diverge_on_different_input() {
        assert_ne!(fnv1a(b"a"), fnv1a(b"b"));
    }
}
