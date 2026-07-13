use super::*;
use bytes::Bytes;
use std::time::Instant;

#[test]
fn cache_key_deterministic_same_fields() {
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "messages": [{"role": "user", "content": "hello"}],
        "temperature": 0.7,
        "max_tokens": 100
    });
    let key1 = cache_key_for_request(
        &body,
        CacheNamespace::Anthropic,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    let key2 = cache_key_for_request(
        &body,
        CacheNamespace::Anthropic,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    assert_eq!(key1, key2);
    assert!(key1.starts_with("anth:"));
}

#[test]
fn cache_key_different_for_different_temperature() {
    let body1 = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "messages": [{"role": "user", "content": "hello"}],
        "temperature": 0.7
    });
    let body2 = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "messages": [{"role": "user", "content": "hello"}],
        "temperature": 0.9
    });
    let key1 = cache_key_for_request(
        &body1,
        CacheNamespace::Anthropic,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    let key2 = cache_key_for_request(
        &body2,
        CacheNamespace::Anthropic,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    assert_ne!(key1, key2);
}

#[test]
fn cache_key_ignores_field_order() {
    // JSON object field order should not affect the key because we
    // extract into a BTreeMap.
    let body1 = serde_json::json!({
        "model": "gpt-4o",
        "temperature": 0.5,
        "messages": [{"role": "user", "content": "hi"}]
    });
    let body2 = serde_json::json!({
        "messages": [{"role": "user", "content": "hi"}],
        "model": "gpt-4o",
        "temperature": 0.5
    });
    let key1 = cache_key_for_request(
        &body1,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    let key2 = cache_key_for_request(
        &body2,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    assert_eq!(key1, key2);
}

#[test]
fn cache_key_ignores_non_cache_fields() {
    let body1 = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "stream": true
    });
    let body2 = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let key1 = cache_key_for_request(
        &body1,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    let key2 = cache_key_for_request(
        &body2,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    assert_eq!(key1, key2);
}

#[test]
fn cache_key_namespace_differs() {
    let body = serde_json::json!({
        "model": "test",
        "messages": []
    });
    let anth = cache_key_for_request(
        &body,
        CacheNamespace::Anthropic,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    let oai = cache_key_for_request(
        &body,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    assert_ne!(anth, oai);
    assert!(anth.starts_with("anth:"));
    assert!(oai.starts_with("oai:"));
}

#[test]
fn cache_key_null_field_same_as_absent() {
    let body1 = serde_json::json!({
        "model": "gpt-4o",
        "messages": [],
        "temperature": null
    });
    let body2 = serde_json::json!({
        "model": "gpt-4o",
        "messages": []
    });
    let key1 = cache_key_for_request(
        &body1,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    let key2 = cache_key_for_request(
        &body2,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    assert_eq!(key1, key2);
}

#[test]
fn parse_cache_ttl_absent() {
    let body = serde_json::json!({"model": "test"});
    assert_eq!(parse_cache_ttl(&body).unwrap(), None);
}

#[test]
fn parse_cache_ttl_null() {
    let body = serde_json::json!({"cache_ttl_secs": null});
    assert_eq!(parse_cache_ttl(&body).unwrap(), None);
}

#[test]
fn parse_cache_ttl_zero() {
    let body = serde_json::json!({"cache_ttl_secs": 0});
    assert_eq!(parse_cache_ttl(&body).unwrap(), Some(0));
}

#[test]
fn parse_cache_ttl_valid() {
    let body = serde_json::json!({"cache_ttl_secs": 600});
    assert_eq!(parse_cache_ttl(&body).unwrap(), Some(600));
}

#[test]
fn parse_cache_ttl_max() {
    let body = serde_json::json!({"cache_ttl_secs": 86400});
    assert_eq!(parse_cache_ttl(&body).unwrap(), Some(86400));
}

#[test]
fn parse_cache_ttl_over_max() {
    let body = serde_json::json!({"cache_ttl_secs": 86401});
    assert!(parse_cache_ttl(&body).is_err());
}

#[test]
fn parse_cache_ttl_negative() {
    let body = serde_json::json!({"cache_ttl_secs": -1});
    assert!(parse_cache_ttl(&body).is_err());
}

#[test]
fn parse_cache_ttl_string() {
    let body = serde_json::json!({"cache_ttl_secs": "not a number"});
    assert!(parse_cache_ttl(&body).is_err());
}

#[test]
fn cache_key_differs_for_different_cache_ttl_secs() {
    let body1 = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "cache_ttl_secs": 60
    });
    let body2 = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "cache_ttl_secs": 3600
    });
    let key1 = cache_key_for_request(
        &body1,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    let key2 = cache_key_for_request(
        &body2,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: None,
        },
    );
    assert_ne!(
        key1, key2,
        "different cache_ttl_secs must produce different cache keys"
    );
}

#[test]
fn cache_key_ignores_litellm_cache_controls() {
    let body1 = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "cache": {"ttl": 60, "no-cache": true}
    });
    let body2 = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "cache": {"ttl": 3600, "no-store": true}
    });

    assert_eq!(openai_key(&body1), openai_key(&body2));
}

#[test]
fn cache_key_namespace_control_separates_keys() {
    let body = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let key1 = cache_key_for_request(
        &body,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: Some("tenant-a"),
        },
    );
    let key2 = cache_key_for_request(
        &body,
        CacheNamespace::OpenAI,
        &CacheScope {
            backend_name: "openai",
            auth_identity: "k1",
            namespace: Some("tenant-b"),
        },
    );

    assert_ne!(key1, key2);
}

#[test]
fn parse_cache_control_litellm_fields() {
    let body = serde_json::json!({
        "cache": {
            "ttl": 120,
            "no-cache": true,
            "no-store": false,
            "s-maxage": 30,
            "namespace": "tenant-a",
            "use-cache": true
        }
    });

    let control = parse_cache_control(&body).unwrap();

    assert!(!control.lookup);
    assert!(control.store);
    assert_eq!(control.ttl_secs, Some(120));
    assert_eq!(control.max_age_secs, Some(30));
    assert_eq!(control.namespace.as_deref(), Some("tenant-a"));
    assert!(control.use_cache);
}

#[test]
fn parse_cache_control_preserves_cache_ttl_secs_bypass() {
    let body = serde_json::json!({
        "cache_ttl_secs": 0,
        "cache": {"ttl": 120}
    });

    let control = parse_cache_control(&body).unwrap();

    assert!(!control.lookup);
    assert!(!control.store);
    assert_eq!(control.ttl_secs, Some(120));
}

#[test]
fn parse_cache_control_rejects_invalid_cache_object() {
    let body = serde_json::json!({"cache": true});

    assert!(parse_cache_control(&body).is_err());
}

#[test]
fn cache_entry_s_maxage_rejects_stale_entries() {
    let entry = CacheEntry {
        response_body: Bytes::from_static(b"{}"),
        model: "gpt-4o".to_string(),
        created_at: Instant::now() - std::time::Duration::from_secs(10),
        ttl_secs: None,
    };

    assert!(!cache_entry_is_fresh(&entry, Some(5)));
    assert!(cache_entry_is_fresh(&entry, Some(30)));
    assert!(cache_entry_is_fresh(&entry, None));
}

fn test_scope() -> CacheScope<'static> {
    CacheScope {
        backend_name: "openai",
        auth_identity: "k1",
        namespace: None,
    }
}

fn anthropic_key(body: &serde_json::Value) -> String {
    cache_key_for_request(body, CacheNamespace::Anthropic, &test_scope())
}

fn openai_key(body: &serde_json::Value) -> String {
    cache_key_for_request(body, CacheNamespace::OpenAI, &test_scope())
}

#[test]
fn cache_key_includes_anthropic_response_affecting_fields() {
    let base = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 128,
        "messages": [{"role": "user", "content": "hi"}]
    });

    let with_top_k = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 128,
        "messages": [{"role": "user", "content": "hi"}],
        "top_k": 10
    });
    let with_stop_sequences = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 128,
        "messages": [{"role": "user", "content": "hi"}],
        "stop_sequences": ["END"]
    });
    let with_thinking = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 128,
        "messages": [{"role": "user", "content": "hi"}],
        "thinking": {"type": "enabled", "budget_tokens": 1024}
    });

    assert_ne!(anthropic_key(&base), anthropic_key(&with_top_k));
    assert_ne!(anthropic_key(&base), anthropic_key(&with_stop_sequences));
    assert_ne!(anthropic_key(&base), anthropic_key(&with_thinking));
}

#[test]
fn cache_key_includes_unknown_extra_fields() {
    let base = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let with_extra = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "prediction": {"type": "content", "content": "expected"}
    });

    assert_ne!(openai_key(&base), openai_key(&with_extra));
}

#[test]
fn cache_key_ignores_tracking_fields() {
    let base = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}]
    });
    let with_user = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "user": "end-user-123"
    });
    let with_metadata = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 128,
        "messages": [{"role": "user", "content": "hi"}],
        "metadata": {"user_id": "session-abc"}
    });
    let metadata_base = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 128,
        "messages": [{"role": "user", "content": "hi"}]
    });

    assert_eq!(openai_key(&base), openai_key(&with_user));
    assert_eq!(anthropic_key(&metadata_base), anthropic_key(&with_metadata));
}

#[test]
fn cache_key_includes_parallel_tool_calls() {
    let base = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "lookup",
                "description": "lookup",
                "parameters": {"type": "object", "properties": {}}
            }
        }]
    });
    let with_parallel_tool_calls = serde_json::json!({
        "model": "gpt-4o",
        "messages": [{"role": "user", "content": "hi"}],
        "tools": [{
            "type": "function",
            "function": {
                "name": "lookup",
                "description": "lookup",
                "parameters": {"type": "object", "properties": {}}
            }
        }],
        "parallel_tool_calls": false
    });

    assert_ne!(openai_key(&base), openai_key(&with_parallel_tool_calls));
}
