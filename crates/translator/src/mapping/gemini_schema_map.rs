// Phase 20d: Gemini schema sanitizer (TASKS.md Phase 20d)
//
// Gemini's FunctionDeclaration.parameters accepts only an OpenAPI 3.0 subset
// of JSON Schema. This module strips unsupported keys and rewrites constructs
// that Gemini rejects.

use serde_json::{Map, Value};

use crate::gemini::tools::FunctionDeclaration;

/// Keys that Gemini rejects in tool parameter schemas.
const UNSUPPORTED_KEYS: &[&str] = &[
    "$schema",
    "$ref",
    "$defs",
    "definitions",
    "default",
    "pattern",
    "examples",
];

/// Format values Gemini supports (OpenAPI 3.0 subset).
const SUPPORTED_FORMATS: &[&str] = &[
    "int32",
    "int64",
    "float",
    "double",
    "date-time",
    "date",
    "time",
    "duration",
    "email",
    "hostname",
    "ipv4",
    "ipv6",
    "uri",
    "uuid",
    "byte",
    "binary",
];

/// Default maximum function declarations per Gemini request.
pub const DEFAULT_FUNCTION_LIMIT: usize = 128;

/// Sanitize a JSON Schema value for Gemini compatibility.
///
/// Recursively strips unsupported keys (`$schema`, `$ref`, `$defs`,
/// `definitions`, `default`, `pattern`, `examples`), rewrites `anyOf`/`oneOf`
/// to the first variant, and removes unsupported `format` values.
///
/// Logs `tracing::warn!` on every lossy transformation.
pub fn clean_gemini_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => clean_object(map),
        other => other.clone(),
    }
}

/// Enforce Gemini's function declaration limit. Truncates and warns if
/// `declarations` exceeds `limit`.
pub fn enforce_function_limit(
    mut declarations: Vec<FunctionDeclaration>,
    limit: usize,
) -> Vec<FunctionDeclaration> {
    if declarations.len() > limit {
        tracing::warn!(
            count = declarations.len(),
            limit,
            "Truncating function declarations to Gemini limit"
        );
        declarations.truncate(limit);
    }
    declarations
}

// -- internal helpers --

fn clean_object(map: &Map<String, Value>) -> Value {
    let mut out = Map::new();

    // Strip unsupported keys
    for (key, value) in map {
        if UNSUPPORTED_KEYS.contains(&key.as_str()) {
            tracing::warn!(key, "Stripping unsupported JSON Schema key for Gemini");
            continue;
        }
        out.insert(key.clone(), value.clone());
    }

    // Rewrite anyOf / oneOf: take first variant, merge into parent
    for keyword in &["anyOf", "oneOf"] {
        if let Some(variants) = out.remove(*keyword) {
            rewrite_union(&mut out, keyword, &variants);
        }
    }

    // Strip unsupported format values
    if let Some(Value::String(fmt)) = out.get("format") {
        if !SUPPORTED_FORMATS.contains(&fmt.as_str()) {
            tracing::warn!(format = %fmt, "Stripping unsupported format value for Gemini");
            out.remove("format");
        }
    }

    // Recurse into nested schema locations
    recurse_key(&mut out, "properties", recurse_properties);
    recurse_key(&mut out, "items", recurse_single);
    recurse_key(&mut out, "additionalProperties", recurse_single);
    recurse_key(&mut out, "not", recurse_single);
    recurse_key(&mut out, "allOf", recurse_array);

    Value::Object(out)
}

/// Rewrite anyOf/oneOf: use first variant or fall back to string type.
fn rewrite_union(out: &mut Map<String, Value>, keyword: &str, variants: &Value) {
    if let Value::Array(arr) = variants {
        if let Some(first) = arr.first() {
            tracing::warn!(
                keyword,
                variant_count = arr.len(),
                "Rewriting {keyword} to first variant for Gemini"
            );
            // Merge the first variant's fields into the parent object
            if let Value::Object(variant_map) = clean_gemini_schema(first) {
                for (k, v) in variant_map {
                    // Don't overwrite keys already present in parent
                    out.entry(k).or_insert(v);
                }
            }
        } else {
            tracing::warn!(keyword, "Empty {keyword}, falling back to string type");
            out.insert("type".to_string(), Value::String("string".to_string()));
        }
    }
}

fn recurse_properties(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, prop_schema) in map {
                out.insert(key.clone(), clean_gemini_schema(prop_schema));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

fn recurse_single(value: &Value) -> Value {
    clean_gemini_schema(value)
}

fn recurse_array(value: &Value) -> Value {
    match value {
        Value::Array(arr) => Value::Array(arr.iter().map(clean_gemini_schema).collect()),
        other => other.clone(),
    }
}

/// Helper: if `key` exists in `map`, replace its value using `transform`.
fn recurse_key(map: &mut Map<String, Value>, key: &str, transform: fn(&Value) -> Value) {
    if let Some(val) = map.remove(key) {
        map.insert(key.to_string(), transform(&val));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn passthrough_valid_schema() {
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "age": {"type": "integer", "format": "int32"}
            },
            "required": ["name"]
        });
        assert_eq!(clean_gemini_schema(&schema), schema);
    }

    #[test]
    fn strip_unsupported_keys() {
        let schema = json!({
            "type": "object",
            "$schema": "http://json-schema.org/draft-07/schema#",
            "$ref": "#/definitions/Foo",
            "$defs": {"Foo": {"type": "string"}},
            "definitions": {"Bar": {"type": "number"}},
            "default": "hello",
            "pattern": "^[a-z]+$",
            "examples": ["foo", "bar"]
        });
        let cleaned = clean_gemini_schema(&schema);
        let obj = cleaned.as_object().unwrap();
        assert_eq!(obj.get("type"), Some(&json!("object")));
        for key in UNSUPPORTED_KEYS {
            assert!(!obj.contains_key(*key), "key {key} should be stripped");
        }
    }

    #[test]
    fn nested_strip_in_properties() {
        let schema = json!({
            "type": "object",
            "properties": {
                "child": {
                    "type": "string",
                    "default": "x",
                    "$ref": "#/defs/Y"
                }
            }
        });
        let cleaned = clean_gemini_schema(&schema);
        let child = &cleaned["properties"]["child"];
        assert_eq!(child, &json!({"type": "string"}));
    }

    #[test]
    fn anyof_rewrite_first_variant() {
        let schema = json!({
            "anyOf": [
                {"type": "string", "description": "A string"},
                {"type": "number"}
            ]
        });
        let cleaned = clean_gemini_schema(&schema);
        let obj = cleaned.as_object().unwrap();
        assert!(!obj.contains_key("anyOf"));
        assert_eq!(obj.get("type"), Some(&json!("string")));
        assert_eq!(obj.get("description"), Some(&json!("A string")));
    }

    #[test]
    fn oneof_rewrite_first_variant() {
        let schema = json!({
            "oneOf": [
                {"type": "integer", "format": "int32"},
                {"type": "string"}
            ]
        });
        let cleaned = clean_gemini_schema(&schema);
        let obj = cleaned.as_object().unwrap();
        assert!(!obj.contains_key("oneOf"));
        assert_eq!(obj.get("type"), Some(&json!("integer")));
        assert_eq!(obj.get("format"), Some(&json!("int32")));
    }

    #[test]
    fn empty_anyof_falls_back_to_string() {
        let schema = json!({"anyOf": []});
        let cleaned = clean_gemini_schema(&schema);
        assert_eq!(cleaned, json!({"type": "string"}));
    }

    #[test]
    fn unsupported_format_stripped() {
        let schema = json!({"type": "string", "format": "custom-thing"});
        let cleaned = clean_gemini_schema(&schema);
        assert_eq!(cleaned, json!({"type": "string"}));
    }

    #[test]
    fn supported_format_kept() {
        let schema = json!({"type": "integer", "format": "int32"});
        assert_eq!(clean_gemini_schema(&schema), schema);
    }

    #[test]
    fn items_recursion() {
        let schema = json!({
            "type": "array",
            "items": {
                "type": "string",
                "default": "x",
                "pattern": "^a"
            }
        });
        let cleaned = clean_gemini_schema(&schema);
        assert_eq!(cleaned["items"], json!({"type": "string"}));
    }

    #[test]
    fn allof_variants_sanitized() {
        let schema = json!({
            "allOf": [
                {"type": "object", "default": "a"},
                {"type": "object", "$ref": "#/foo"}
            ]
        });
        let cleaned = clean_gemini_schema(&schema);
        let arr = cleaned["allOf"].as_array().unwrap();
        assert_eq!(arr[0], json!({"type": "object"}));
        assert_eq!(arr[1], json!({"type": "object"}));
    }

    #[test]
    fn enforce_function_limit_truncates() {
        let decls: Vec<FunctionDeclaration> = (0..130)
            .map(|i| FunctionDeclaration {
                name: format!("fn_{i}"),
                description: None,
                parameters: None,
            })
            .collect();
        let result = enforce_function_limit(decls, DEFAULT_FUNCTION_LIMIT);
        assert_eq!(result.len(), 128);
        assert_eq!(result.last().unwrap().name, "fn_127");
    }

    #[test]
    fn enforce_function_limit_passthrough() {
        let decls: Vec<FunctionDeclaration> = (0..50)
            .map(|i| FunctionDeclaration {
                name: format!("fn_{i}"),
                description: None,
                parameters: None,
            })
            .collect();
        let result = enforce_function_limit(decls, DEFAULT_FUNCTION_LIMIT);
        assert_eq!(result.len(), 50);
    }

    #[test]
    fn complex_real_world_schema() {
        // Nested object with multiple issues
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "pattern": "^SELECT",
                    "default": "*"
                },
                "options": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "limit": {"type": "integer", "format": "int32", "default": 10}
                            }
                        },
                        {"type": "string"}
                    ]
                },
                "tags": {
                    "type": "array",
                    "items": {
                        "type": "string",
                        "format": "custom-tag",
                        "examples": ["foo"]
                    }
                }
            },
            "required": ["query"],
            "$defs": {
                "Helper": {"type": "string"}
            }
        });
        let cleaned = clean_gemini_schema(&schema);
        let obj = cleaned.as_object().unwrap();

        // Top-level: $schema, $defs stripped
        assert!(!obj.contains_key("$schema"));
        assert!(!obj.contains_key("$defs"));
        assert_eq!(obj["type"], "object");
        assert_eq!(obj["required"], json!(["query"]));

        // query: pattern and default stripped
        assert_eq!(cleaned["properties"]["query"], json!({"type": "string"}));

        // options: oneOf rewritten to first variant, nested default stripped
        let options = &cleaned["properties"]["options"];
        assert!(!options.as_object().unwrap().contains_key("oneOf"));
        assert_eq!(options["type"], "object");
        assert_eq!(
            options["properties"]["limit"],
            json!({"type": "integer", "format": "int32"})
        );

        // tags: items has format and examples stripped
        assert_eq!(
            cleaned["properties"]["tags"]["items"],
            json!({"type": "string"})
        );
    }
}
