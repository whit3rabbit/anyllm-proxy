use serde_json::{json, Value};
use std::collections::HashSet;

/// Word-level Jaccard similarity in [0,1]. 1.0 if both empty.
pub fn jaccard(a: &str, b: &str) -> f64 {
    let sa: HashSet<&str> = a.split_whitespace().collect();
    let sb: HashSet<&str> = b.split_whitespace().collect();
    if sa.is_empty() && sb.is_empty() {
        return 1.0;
    }
    let inter = sa.intersection(&sb).count() as f64;
    let uni = sa.union(&sb).count() as f64;
    if uni == 0.0 {
        1.0
    } else {
        inter / uni
    }
}

pub fn wrap_prompt(p: &str) -> Value {
    json!({ "messages": [ { "role": "user", "content": p } ] })
}
