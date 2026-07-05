//! Structure-aware JSON-Schema annotation stripper. Port of pxpipe's
//! `schema-strip.ts`.
//!
//! The point is that the literal key `description` (also `title`, `default`,
//! `examples`) is a schema ANNOTATION in one place and a user-defined PROPERTY
//! NAME in another (the `task` tool has a required param literally named
//! `description`). A naive "drop every key called description" walk deletes that
//! property, leaving `required: ["description"]` dangling and breaking the tool
//! call. So we strip annotation keywords only at the schema-node level and
//! recurse into the *values* of `properties`/`$defs`/etc., never treating their
//! keys as annotations.

use serde_json::{Map, Value};

const MAX_DEPTH: usize = 20;

/// Annotation keywords: tokens but no validation. Stripped at node level.
const ANNOTATIONS: &[&str] = &[
    "description",
    "title",
    "examples",
    "default",
    "$schema",
    "$id",
    "$comment",
];
/// Keys whose value is a map of *named subschemas* — recurse into values only.
const SUBSCHEMA_MAPS: &[&str] = &["properties", "$defs", "definitions", "patternProperties"];
/// Keys whose value is a single subschema.
const SUBSCHEMA_ONE: &[&str] = &[
    "items",
    "additionalProperties",
    "not",
    "if",
    "then",
    "else",
    "contains",
    "propertyNames",
];
/// Keys whose value is an array of subschemas.
const SUBSCHEMA_ARRAY: &[&str] = &["oneOf", "anyOf", "allOf"];
/// Contract keys that must survive untouched (structural validation).
const CONTRACT: &[&str] = &["required", "enum", "const", "type", "$ref"];

/// Return a fresh schema with long-form metadata removed, structure preserved.
/// Never mutates the input.
pub fn strip(node: &Value) -> Value {
    strip_at(node, 0)
}

fn strip_at(node: &Value, depth: usize) -> Value {
    if depth > MAX_DEPTH {
        return node.clone();
    }
    let Value::Object(obj) = node else {
        return node.clone();
    };
    let mut out = Map::new();
    for (k, v) in obj {
        if ANNOTATIONS.contains(&k.as_str()) {
            continue;
        }
        // `format` beyond a short token is a description in disguise.
        if k == "format" {
            if let Some(s) = v.as_str() {
                if s.len() <= 20 {
                    out.insert(k.clone(), v.clone());
                }
            }
            continue;
        }
        if CONTRACT.contains(&k.as_str()) {
            out.insert(k.clone(), v.clone());
        } else if SUBSCHEMA_MAPS.contains(&k.as_str()) {
            if let Value::Object(m) = v {
                let mut nm = Map::new();
                for (pk, pv) in m {
                    nm.insert(pk.clone(), strip_at(pv, depth + 1)); // key = property NAME, keep
                }
                out.insert(k.clone(), Value::Object(nm));
            } else {
                out.insert(k.clone(), v.clone());
            }
        } else if SUBSCHEMA_ARRAY.contains(&k.as_str()) {
            if let Value::Array(a) = v {
                out.insert(
                    k.clone(),
                    Value::Array(a.iter().map(|e| strip_at(e, depth + 1)).collect()),
                );
            } else {
                out.insert(k.clone(), v.clone());
            }
        } else if SUBSCHEMA_ONE.contains(&k.as_str()) {
            out.insert(k.clone(), strip_at(v, depth + 1));
        } else {
            // Unknown key — recurse into nested objects to strip vendor-ext
            // descriptions, otherwise pass through.
            out.insert(k.clone(), strip_at(v, depth + 1));
        }
    }
    Value::Object(out)
}

/// True when the stripped schema still carries a validation contract — else the
/// strip isn't worth shipping (a bare `{type:object}` stub caused 400s).
pub fn has_structure(schema: &Value) -> bool {
    let Some(obj) = schema.as_object() else {
        return false;
    };
    obj.keys().any(|k| {
        matches!(
            k.as_str(),
            "properties"
                | "required"
                | "enum"
                | "const"
                | "items"
                | "$ref"
                | "oneOf"
                | "anyOf"
                | "allOf"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn strips_annotations_keeps_structure() {
        let schema = json!({
            "type": "object",
            "description": "long prose that costs tokens",
            "properties": {
                "path": { "type": "string", "description": "the file path" }
            },
            "required": ["path"]
        });
        let out = strip(&schema);
        assert!(out.get("description").is_none());
        assert_eq!(out["required"], json!(["path"]));
        assert!(out["properties"]["path"].get("description").is_none());
        assert_eq!(out["properties"]["path"]["type"], json!("string"));
    }

    #[test]
    fn preserves_property_named_description() {
        // The `task` tool bug: a property literally named "description".
        let schema = json!({
            "type": "object",
            "properties": {
                "description": { "type": "string", "description": "annotation to strip" }
            },
            "required": ["description"]
        });
        let out = strip(&schema);
        assert!(
            out["properties"].get("description").is_some(),
            "property must survive"
        );
        assert!(out["properties"]["description"]
            .get("description")
            .is_none());
        assert_eq!(out["required"], json!(["description"]));
    }

    #[test]
    fn structure_detection() {
        assert!(has_structure(&json!({"type":"object","properties":{}})));
        assert!(!has_structure(&json!({"type":"object"})));
    }
}
