//! Byte encodings and string forms for vectors, metrics and index methods.

use super::*;

/// Encode a `Vec<f32>` as little-endian bytes for sqlite-vec MATCH parameters.
pub(super) fn encode_vec(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

/// Encode a [`SparseVector`] to a self-describing little-endian blob:
/// `u32 len` then `len` ascending `u32` indices then `len` `f32` values.
pub(super) fn encode_sparse(sv: &SparseVector) -> Vec<u8> {
    let n = sv.indices.len();
    let mut buf = Vec::with_capacity(4 + n * 8);
    buf.extend_from_slice(&(n as u32).to_le_bytes());
    for &i in &sv.indices {
        buf.extend_from_slice(&i.to_le_bytes());
    }
    for &v in &sv.values {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf
}

/// Decode a blob written by [`encode_sparse`]. Returns `None` on any length or
/// truncation mismatch so a corrupt row is skipped rather than panicking.
pub(super) fn decode_sparse(bytes: &[u8]) -> Option<SparseVector> {
    let n = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?) as usize;
    let expected = 4usize.checked_add(n.checked_mul(8)?)?;
    if bytes.len() != expected {
        return None;
    }
    let mut indices = Vec::with_capacity(n);
    let mut values = Vec::with_capacity(n);
    for k in 0..n {
        let off = 4 + k * 4;
        indices.push(u32::from_le_bytes(
            bytes.get(off..off + 4)?.try_into().ok()?,
        ));
    }
    let val_start = 4 + n * 4;
    for k in 0..n {
        let off = val_start + k * 4;
        values.push(f32::from_le_bytes(
            bytes.get(off..off + 4)?.try_into().ok()?,
        ));
    }
    Some(SparseVector { indices, values })
}

/// Encode a [`MultiVector`] to a self-describing little-endian blob:
/// `u32 num_tokens`, `u32 dim`, then `num_tokens * dim` `f32` values.
pub(super) fn encode_multi_vector(mv: &MultiVector) -> Vec<u8> {
    let mut buf = Vec::with_capacity(8 + mv.data.len() * 4);
    buf.extend_from_slice(&mv.num_tokens.to_le_bytes());
    buf.extend_from_slice(&mv.dim.to_le_bytes());
    for &v in &mv.data {
        buf.extend_from_slice(&v.to_le_bytes());
    }
    buf
}

/// Decode a blob written by [`encode_multi_vector`]. Returns `None` on any
/// length or truncation mismatch so a corrupt row is skipped, not panicked on.
pub(super) fn decode_multi_vector(bytes: &[u8]) -> Option<MultiVector> {
    let num_tokens = u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?);
    let dim = u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?);
    let count = (num_tokens as usize).checked_mul(dim as usize)?;
    let expected = 8usize.checked_add(count.checked_mul(4)?)?;
    if bytes.len() != expected {
        return None;
    }
    let mut data = Vec::with_capacity(count);
    for k in 0..count {
        let off = 8 + k * 4;
        data.push(f32::from_le_bytes(
            bytes.get(off..off + 4)?.try_into().ok()?,
        ));
    }
    Some(MultiVector {
        num_tokens,
        dim,
        data,
    })
}

/// Map `DistanceMetric` to the sqlite-vec column option string.
///
/// sqlite-vec supports `l2`, `l1`, and `cosine`. `InnerProduct` is not natively
/// supported; we fall back to `cosine`, which is equivalent for unit-normalised
/// vectors and is the typical use case for inner-product similarity.
pub(super) fn metric_str(m: DistanceMetric) -> &'static str {
    match m {
        DistanceMetric::Cosine | DistanceMetric::InnerProduct => "cosine",
        DistanceMetric::L2 => "l2",
    }
}

pub(super) fn parse_metric(s: &str) -> DistanceMetric {
    match s {
        "l2" => DistanceMetric::L2,
        _ => DistanceMetric::Cosine,
    }
}

pub(super) fn parse_index_method(s: &str) -> IndexMethod {
    match s {
        "hnsw" => IndexMethod::Hnsw,
        _ => IndexMethod::Flat,
    }
}

pub(super) fn index_method_str(m: IndexMethod) -> &'static str {
    match m {
        IndexMethod::Hnsw => "hnsw",
        IndexMethod::Flat | IndexMethod::Diskann => "flat",
    }
}
