// Integration tests for batch processing endpoints (T026-T036).
// Tests file upload, batch creation, status retrieval, listing, and 501 on unsupported backends.

use anyllm_proxy::admin;
use anyllm_proxy::config::{self, Config, MultiConfig};
use anyllm_proxy::server::routes;
use reqwest::{multipart, Client};

fn test_config() -> Config {
    Config {
        backend: config::BackendKind::OpenAI,
        openai_api_key: "test-key".to_string(),
        openai_base_url: "https://api.openai.com".to_string(),
        listen_port: 0,
        model_mapping: config::ModelMapping {
            big_model: "gpt-4o".into(),
            small_model: "gpt-4o-mini".into(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: config::BackendAuth::BearerToken("test-key".into()),
        log_bodies: false,
        expose_degradation_warnings: false,
        openai_api_format: config::OpenAIApiFormat::Chat,
    }
}

/// Spawn a test server with SharedState (needed for batch DB access).
async fn spawn_test_server_with_shared() -> String {
    std::env::set_var("PROXY_OPEN_RELAY", "true");
    let config = test_config();
    let multi = MultiConfig::from_single_config(&config);
    let shared = admin::state::SharedState::new_for_test();

    // Initialize batch tables in the test DB
    {
        let conn = shared.db.lock().unwrap();
        anyllm_proxy::batch::db::init_batch_tables(&conn).unwrap();
    }

    let app = routes::app_multi_with_shared(multi, Some(shared), None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{addr}")
}

fn valid_jsonl() -> &'static str {
    r#"{"custom_id": "req-1", "body": {"model": "gpt-4o", "messages": [{"role": "user", "content": "Hello"}]}}
{"custom_id": "req-2", "body": {"model": "gpt-4o", "messages": [{"role": "user", "content": "World"}]}}"#
}

#[tokio::test]
async fn upload_file_and_create_batch() {
    let base = spawn_test_server_with_shared().await;
    let client = Client::new();

    // Step 1: Upload a valid JSONL file
    let form = multipart::Form::new().text("purpose", "batch").part(
        "file",
        multipart::Part::bytes(valid_jsonl().as_bytes().to_vec())
            .file_name("test.jsonl")
            .mime_str("application/jsonl")
            .unwrap(),
    );

    let resp = client
        .post(format!("{base}/v1/files"))
        .header("x-api-key", "test")
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let file_obj: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(file_obj["object"], "file");
    assert_eq!(file_obj["purpose"], "batch");
    assert!(file_obj["id"].as_str().unwrap().starts_with("file-"));
    let file_id = file_obj["id"].as_str().unwrap().to_string();

    // Step 2: Create a batch job
    let resp = client
        .post(format!("{base}/v1/batches"))
        .header("x-api-key", "test")
        .json(&serde_json::json!({
            "input_file_id": file_id,
            "endpoint": "/v1/chat/completions",
            "completion_window": "24h"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    let batch_obj: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(batch_obj["object"], "batch");
    assert!(batch_obj["id"].as_str().unwrap().starts_with("batch-"));
    assert_eq!(batch_obj["status"], "validating");
    assert_eq!(batch_obj["input_file_id"], file_id);
    assert_eq!(batch_obj["request_counts"]["total"], 2);
    let batch_id = batch_obj["id"].as_str().unwrap().to_string();

    // Step 3: Get batch status
    let resp = client
        .get(format!("{base}/v1/batches/{batch_id}"))
        .header("x-api-key", "test")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let fetched: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(fetched["id"], batch_id);

    // Step 4: List batches
    let resp = client
        .get(format!("{base}/v1/batches"))
        .header("x-api-key", "test")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let list: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(list["object"], "list");
    assert!(!list["data"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn upload_invalid_jsonl_returns_400() {
    let base = spawn_test_server_with_shared().await;
    let client = Client::new();

    let form = multipart::Form::new().text("purpose", "batch").part(
        "file",
        multipart::Part::bytes(b"not valid json".to_vec()).file_name("bad.jsonl"),
    );

    let resp = client
        .post(format!("{base}/v1/files"))
        .header("x-api-key", "test")
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn create_batch_with_missing_file_returns_400() {
    let base = spawn_test_server_with_shared().await;
    let client = Client::new();

    let resp = client
        .post(format!("{base}/v1/batches"))
        .header("x-api-key", "test")
        .json(&serde_json::json!({
            "input_file_id": "file-nonexistent",
            "endpoint": "/v1/chat/completions",
            "completion_window": "24h"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}

#[tokio::test]
async fn get_nonexistent_batch_returns_404() {
    let base = spawn_test_server_with_shared().await;
    let client = Client::new();

    let resp = client
        .get(format!("{base}/v1/batches/batch-does-not-exist"))
        .header("x-api-key", "test")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn unsupported_backend_returns_501() {
    std::env::set_var("PROXY_OPEN_RELAY", "true");

    // Create a config with an Anthropic backend (unsupported for batches)
    let config = Config {
        backend: config::BackendKind::Anthropic,
        openai_api_key: "test-key".to_string(),
        openai_base_url: "https://api.anthropic.com".to_string(),
        listen_port: 0,
        model_mapping: config::ModelMapping {
            big_model: "claude-sonnet-4-6".into(),
            small_model: "claude-haiku-4-5".into(),
        },
        tls: config::TlsConfig::default(),
        backend_auth: config::BackendAuth::BearerToken("test-key".into()),
        log_bodies: false,
        expose_degradation_warnings: false,
        openai_api_format: config::OpenAIApiFormat::Chat,
    };

    let multi = MultiConfig::from_single_config(&config);
    let shared = admin::state::SharedState::new_for_test();
    {
        let conn = shared.db.lock().unwrap();
        anyllm_proxy::batch::db::init_batch_tables(&conn).unwrap();
    }
    let app = routes::app_multi_with_shared(multi, Some(shared), None);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let base = format!("http://{addr}");

    let client = Client::new();
    let resp = client
        .post(format!("{base}/v1/batches"))
        .header("x-api-key", "test")
        .json(&serde_json::json!({
            "input_file_id": "file-abc",
            "endpoint": "/v1/chat/completions",
            "completion_window": "24h"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 501);
}

#[tokio::test]
async fn anthropic_batch_rejects_empty_requests() {
    let base = spawn_test_server_with_shared().await;
    let client = Client::new();

    let resp = client
        .post(format!("{base}/v1/messages/batches"))
        .header("x-api-key", "test")
        .header("content-type", "application/json")
        .body(serde_json::to_string(&serde_json::json!({"requests": []})).unwrap())
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400);
}
