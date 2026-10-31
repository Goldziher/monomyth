//! In-process evaluation of a [`Filter`] against a stored document and chunk.

use super::*;

pub(super) fn resolve_field_value(
    field: &FilterField,
    doc: &DocumentRecord,
    chunk: &ChunkRecord,
) -> Option<serde_json::Value> {
    let parsed = field.parse().ok()?;
    match parsed.namespace {
        FilterNamespace::Doc => match parsed.path.as_str() {
            "full_text" => Some(serde_json::Value::String(doc.full_text.clone())),
            "title" => doc.title.clone().map(serde_json::Value::String),
            "mime" => doc.mime.clone().map(serde_json::Value::String),
            "external_id" => doc.external_id.clone().map(serde_json::Value::String),
            "source_uri" => doc.source_uri.clone().map(serde_json::Value::String),
            "keywords" => serde_json::to_value(&doc.keywords).ok(),
            "labels" => Some(doc.labels.clone()),
            "entities" => Some(doc.entities.clone()),
            "ingested_at" => None,
            path if path.starts_with("metadata.") => {
                json_pointer(&doc.metadata, &path["metadata.".len()..])
            }
            _ => None,
        },
        FilterNamespace::Chunk => match parsed.path.as_str() {
            "content" => Some(serde_json::Value::String(chunk.content.clone())),
            "ordinal" => Some(serde_json::Value::from(chunk.ordinal)),
            "external_id" => chunk.external_id.clone().map(serde_json::Value::String),
            path if path.starts_with("chunk_metadata.") => {
                json_pointer(&chunk.chunk_metadata, &path["chunk_metadata.".len()..])
            }
            _ => None,
        },
    }
}

pub(super) fn json_pointer(value: &serde_json::Value, dotted: &str) -> Option<serde_json::Value> {
    let mut cur = value;
    for seg in dotted.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur.clone())
}

pub(super) fn json_cmp(a: &serde_json::Value, b: &serde_json::Value) -> Option<std::cmp::Ordering> {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x.partial_cmp(&y),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => Some(x.cmp(y)),
            _ => None,
        },
    }
}

pub(super) fn eval_filter(filter: &Filter, doc: &DocumentRecord, chunk: &ChunkRecord) -> bool {
    match filter {
        Filter::Eq { field, value } => {
            resolve_field_value(field, doc, chunk).as_ref() == Some(value)
        }
        Filter::In { field, values } => {
            resolve_field_value(field, doc, chunk).is_some_and(|v| values.iter().any(|c| c == &v))
        }
        Filter::ArrayContains { field, value } => resolve_field_value(field, doc, chunk)
            .and_then(|v| v.as_array().cloned())
            .is_some_and(|arr| arr.iter().any(|item| item == value)),
        Filter::Range {
            field,
            gte,
            gt,
            lte,
            lt,
        } => {
            let Some(v) = resolve_field_value(field, doc, chunk) else {
                return false;
            };
            use std::cmp::Ordering;
            let pass = |bound: &Option<serde_json::Value>, want: &[Ordering]| {
                bound
                    .as_ref()
                    .is_none_or(|b| json_cmp(&v, b).is_some_and(|ord| want.contains(&ord)))
            };
            pass(gte, &[Ordering::Greater, Ordering::Equal])
                && pass(gt, &[Ordering::Greater])
                && pass(lte, &[Ordering::Less, Ordering::Equal])
                && pass(lt, &[Ordering::Less])
        }
        Filter::TextMatch { field, query } => resolve_field_value(field, doc, chunk)
            .and_then(|v| v.as_str().map(str::to_lowercase))
            .is_some_and(|s| s.contains(&query.to_lowercase())),
        Filter::And { filters } => filters.iter().all(|f| eval_filter(f, doc, chunk)),
        Filter::Or { filters } => filters.iter().any(|f| eval_filter(f, doc, chunk)),
        Filter::Not { filter } => !eval_filter(filter, doc, chunk),
    }
}
