//! Schema-guided repair for structured model output (roadmap 1g).
//!
//! A JSON-schema-constrained provider still occasionally returns a value whose
//! *shape* matches the schema but whose primitive *types* do not: a numeric
//! field returned as a quoted string (`"5"` for a `u32`), a boolean returned
//! as `"true"`, a schema-typed string field returned as a bare number, or a
//! single object returned where the schema wants a one-element array. Each of
//! these fails `serde_json::from_value::<T>` outright even though the intended
//! value is unambiguous from the schema alone.
//!
//! [`repair_against_schema`] walks the parsed value alongside the same
//! `schemars` JSON Schema passed to the provider (pre-sanitization, so
//! `$ref`/`$defs` are still present) and coerces exactly those mismatches,
//! leaving everything else untouched. It never invents or drops a field, so a
//! value it cannot repair — a missing required field, a genuinely wrong shape —
//! passes through unchanged and still fails deserialization, entering the
//! existing retry loop rather than being silently guessed at.

use std::collections::BTreeSet;

use serde_json::Value;

/// Coerce primitive-type mismatches in `value` against `root` (the full,
/// un-sanitized schema for the target type, as produced by `schema_for!`).
pub(crate) fn repair_against_schema(value: Value, root: &Value) -> Value {
    let mut active = BTreeSet::new();
    repair_node(value, root, root, &mut active)
}

/// Repair `value` against `node` (a schema node reachable from `root`),
/// resolving `$ref`s against `root`'s `$defs`/`definitions` pool. `active`
/// guards against a cyclic `$ref` by refusing to re-enter a definition already
/// being expanded on the current path.
fn repair_node(value: Value, root: &Value, node: &Value, active: &mut BTreeSet<String>) -> Value {
    let Value::Object(schema) = node else {
        return value;
    };

    if let Some(Value::String(reference)) = schema.get("$ref") {
        return match resolve_ref(reference, root) {
            Some((name, resolved)) if !active.contains(&name) => {
                active.insert(name.clone());
                let repaired = repair_node(value, root, resolved, active);
                active.remove(&name);
                repaired
            }
            _ => value,
        };
    }

    let types = schema_types(schema);

    // A schema expecting an array but given a bare scalar/object is wrapped as ~keep
    // a single-element array before recursing into the item schema. ~keep
    if types.contains(&"array") && !value.is_array() && !value.is_null() {
        let item_schema = schema.get("items");
        let item = match item_schema {
            Some(item_schema) => repair_node(value, root, item_schema, active),
            None => value,
        };
        return Value::Array(vec![item]);
    }

    match value {
        Value::Object(mut map) => {
            if let Some(Value::Object(properties)) = schema.get("properties") {
                for (key, prop_schema) in properties {
                    if let Some(field_value) = map.remove(key) {
                        map.insert(
                            key.clone(),
                            repair_node(field_value, root, prop_schema, active),
                        );
                    }
                }
            }
            Value::Object(map)
        }
        Value::Array(items) => match schema.get("items") {
            Some(item_schema) => Value::Array(
                items
                    .into_iter()
                    .map(|item| repair_node(item, root, item_schema, active))
                    .collect(),
            ),
            None => Value::Array(items),
        },
        scalar => coerce_scalar(scalar, &types),
    }
}

/// Resolve a local `#/$defs/<name>` or `#/definitions/<name>` reference
/// against `root`, returning the definition's name (for the `active` cycle
/// guard) alongside its schema node.
fn resolve_ref<'a>(reference: &str, root: &'a Value) -> Option<(String, &'a Value)> {
    let name = reference
        .strip_prefix("#/$defs/")
        .or_else(|| reference.strip_prefix("#/definitions/"))?;
    let defs = root.get("$defs").or_else(|| root.get("definitions"))?;
    let resolved = defs.get(name)?;
    Some((name.to_owned(), resolved))
}

/// The JSON Schema `type` keyword's value, normalized to a list: a bare
/// string (`"type": "integer"`) or an array (`"type": ["string", "null"]`,
/// schemars' nullable-field shape). Empty when the keyword is absent.
fn schema_types(schema: &serde_json::Map<String, Value>) -> Vec<&str> {
    match schema.get("type") {
        Some(Value::String(single)) => vec![single.as_str()],
        Some(Value::Array(many)) => many.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// Coerce a scalar `value` toward the first `types` entry it can be
/// unambiguously read as; returns `value` unchanged when no coercion applies
/// or the coercion itself fails to parse.
fn coerce_scalar(value: Value, types: &[&str]) -> Value {
    let Value::String(text) = &value else {
        return match &value {
            Value::Number(_) | Value::Bool(_) if types.contains(&"string") => {
                Value::String(value.to_string())
            }
            _ => value,
        };
    };
    let trimmed = text.trim();
    if types.contains(&"integer")
        && let Ok(parsed) = trimmed.parse::<i64>()
    {
        return Value::from(parsed);
    }
    if types.contains(&"number")
        && let Ok(parsed) = trimmed.parse::<f64>()
        && let Some(number) = serde_json::Number::from_f64(parsed)
    {
        return Value::Number(number);
    }
    if types.contains(&"boolean") {
        match trimmed.to_ascii_lowercase().as_str() {
            "true" => return Value::Bool(true),
            "false" => return Value::Bool(false),
            _ => {}
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use schemars::{JsonSchema, schema_for};
    use serde::Deserialize;
    use serde_json::json;

    use super::repair_against_schema;

    #[derive(Debug, Deserialize, JsonSchema, PartialEq)]
    struct Hero {
        name: String,
        level: u32,
        active: bool,
        nickname: Option<String>,
        tags: Vec<String>,
    }

    fn hero_schema() -> serde_json::Value {
        serde_json::to_value(schema_for!(Hero)).expect("schema serializes")
    }

    #[test]
    fn coerces_a_stringified_integer_field() {
        let schema = hero_schema();
        let value = json!({
            "name": "Gilgamesh",
            "level": "5",
            "active": true,
            "nickname": null,
            "tags": []
        });
        let repaired = repair_against_schema(value, &schema);
        let hero: Hero = serde_json::from_value(repaired).expect("repaired value deserializes");
        assert_eq!(hero.level, 5);
    }

    #[test]
    fn coerces_a_stringified_boolean_field() {
        let schema = hero_schema();
        let value = json!({
            "name": "Gilgamesh",
            "level": 5,
            "active": "true",
            "nickname": null,
            "tags": []
        });
        let repaired = repair_against_schema(value, &schema);
        let hero: Hero = serde_json::from_value(repaired).expect("repaired value deserializes");
        assert!(hero.active);
    }

    #[test]
    fn coerces_a_bare_number_into_a_schema_typed_string_field() {
        let schema = hero_schema();
        let value = json!({
            "name": 12345,
            "level": 5,
            "active": true,
            "nickname": null,
            "tags": []
        });
        let repaired = repair_against_schema(value, &schema);
        let hero: Hero = serde_json::from_value(repaired).expect("repaired value deserializes");
        assert_eq!(hero.name, "12345");
    }

    #[test]
    fn wraps_a_bare_string_into_a_one_element_array_for_a_vec_field() {
        let schema = hero_schema();
        let value = json!({
            "name": "Gilgamesh",
            "level": 5,
            "active": true,
            "nickname": null,
            "tags": "solo"
        });
        let repaired = repair_against_schema(value, &schema);
        let hero: Hero = serde_json::from_value(repaired).expect("repaired value deserializes");
        assert_eq!(hero.tags, vec!["solo".to_owned()]);
    }

    #[test]
    fn leaves_a_null_option_field_untouched() {
        let schema = hero_schema();
        let value = json!({
            "name": "Gilgamesh",
            "level": 5,
            "active": true,
            "nickname": null,
            "tags": []
        });
        let repaired = repair_against_schema(value, &schema);
        let hero: Hero = serde_json::from_value(repaired).expect("repaired value deserializes");
        assert_eq!(hero.nickname, None);
    }

    #[test]
    fn a_missing_required_field_is_not_invented() {
        let schema = hero_schema();
        let value = json!({
            "name": "Gilgamesh",
            "active": true,
            "nickname": null,
            "tags": []
        });
        let repaired = repair_against_schema(value, &schema);
        assert!(
            serde_json::from_value::<Hero>(repaired).is_err(),
            "repair must never fabricate a missing field"
        );
    }

    #[derive(Debug, Deserialize, JsonSchema, PartialEq)]
    struct Item {
        name: String,
        score: f64,
    }

    #[derive(Debug, Deserialize, JsonSchema, PartialEq)]
    struct Roster {
        items: Vec<Item>,
    }

    #[test]
    fn resolves_refs_to_repair_nested_object_fields() {
        let schema = serde_json::to_value(schema_for!(Roster)).expect("schema serializes");
        let value = json!({ "items": [{ "name": "Enkidu", "score": "9.5" }] });
        let repaired = repair_against_schema(value, &schema);
        let roster: Roster = serde_json::from_value(repaired).expect("repaired value deserializes");
        assert!((roster.items[0].score - 9.5).abs() < f64::EPSILON);
    }

    #[test]
    fn wraps_a_single_ref_object_into_a_one_element_array() {
        let schema = serde_json::to_value(schema_for!(Roster)).expect("schema serializes");
        let value = json!({ "items": { "name": "Enkidu", "score": 9.5 } });
        let repaired = repair_against_schema(value, &schema);
        let roster: Roster = serde_json::from_value(repaired).expect("repaired value deserializes");
        assert_eq!(roster.items.len(), 1);
        assert_eq!(roster.items[0].name, "Enkidu");
    }

    #[test]
    fn does_not_loop_forever_on_a_cyclic_ref() {
        let schema = json!({
            "type": "object",
            "properties": { "self_ref": { "$ref": "#/$defs/Node" } },
            "$defs": {
                "Node": {
                    "type": "object",
                    "properties": { "next": { "$ref": "#/$defs/Node" } }
                }
            }
        });
        let value = json!({ "self_ref": { "next": { "next": {} } } });
        // Must terminate; the exact output shape is secondary to not hanging. ~keep
        let repaired = repair_against_schema(value.clone(), &schema);
        assert_eq!(
            repaired, value,
            "an unrepairable cyclic shape passes through unchanged"
        );
    }
}
