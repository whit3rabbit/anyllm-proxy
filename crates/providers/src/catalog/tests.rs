use super::*;
use crate::model::ModelStatus;
use crate::provider::{ProviderCapabilities, ProviderStatus};

const TEST_FIXTURE: &str = r#"{
      "openai/gpt-fresh": {
        "litellm_provider": "openai",
        "mode": "chat",
        "max_input_tokens": 12345,
        "max_output_tokens": 678,
        "supports_function_calling": true,
        "supports_vision": true,
        "supports_reasoning": true
      },
      "newco/new-model": {
        "litellm_provider": "newco",
        "mode": "chat",
        "max_input_tokens": 4000,
        "max_output_tokens": 500,
        "supports_tool_choice": true,
        "deprecation_date": "2026-01-01"
      },
      "newco/embed-v1": {
        "litellm_provider": "newco",
        "mode": "embedding",
        "max_input_tokens": 8192,
        "max_output_tokens": 0
      },
      "sample_spec": {
        "litellm_provider": "one of https://docs.litellm.ai/docs/providers"
      }
    }"#;

#[test]
fn bundled_catalog_matches_static_lookup_shape() {
    let catalog = ProviderCatalog::bundled();

    assert_eq!(
        catalog.all_providers().count(),
        crate::registry::all_providers().count()
    );
    assert_eq!(
        catalog.get_provider("gmi_cloud").unwrap().id,
        crate::registry::get_provider("gmi_cloud").unwrap().id
    );
    assert_eq!(
        catalog.list_models("zhipuai").len(),
        crate::registry::list_models("zhipuai").len()
    );
    assert!(catalog
        .all_providers()
        .all(|provider| provider.id != "lm_studio"));
    assert!(catalog.get_provider("lm_studio").is_some());
}

#[test]
fn litellm_overlay_preserves_known_provider_metadata() {
    let catalog = ProviderCatalog::from_litellm_json(TEST_FIXTURE).unwrap();
    let provider = catalog.get_provider("openai").unwrap();
    let static_provider = crate::registry::get_provider("openai").unwrap();

    assert_eq!(provider.default_base_url, static_provider.default_base_url);
    assert_eq!(provider.protocol, static_provider.protocol);
    assert_eq!(provider.auth, static_provider.auth);
    assert_eq!(provider.env_vars, static_provider.env_vars);
    assert!(provider.capabilities.chat_completions);
    assert!(provider.capabilities.tool_use);
    assert!(provider.capabilities.vision);
}

#[test]
fn litellm_overlay_adds_new_provider_without_base_url() {
    let catalog = ProviderCatalog::from_litellm_json(TEST_FIXTURE).unwrap();
    let provider = catalog.get_provider("newco").unwrap();

    assert_eq!(provider.status, ProviderStatus::Stub);
    assert_eq!(provider.default_base_url, "");
    assert_eq!(provider.env_vars, vec!["NEWCO_API_KEY"]);
    assert_eq!(provider.litellm_prefix, "newco/");
    assert!(provider.capabilities.chat_completions);
    assert!(provider.capabilities.embeddings);
    assert!(provider.capabilities.tool_use);

    let (kind, base_url) = catalog.resolve_backend("newco").unwrap();
    assert_eq!(kind, "openai");
    assert_eq!(base_url, "");
}

#[test]
fn litellm_overlay_normalizes_models_and_capabilities() {
    let catalog = ProviderCatalog::from_litellm_json(TEST_FIXTURE).unwrap();
    let openai_model = catalog.get_model("openai", "gpt-fresh").unwrap();
    assert_eq!(openai_model.context_window, 12345);
    assert_eq!(openai_model.max_output_tokens, 678);
    assert!(openai_model.capabilities.streaming);
    assert!(openai_model.capabilities.tool_use);
    assert!(openai_model.capabilities.vision);
    assert!(openai_model.capabilities.extended_thinking);

    let new_model = catalog.get_model("newco", "new-model").unwrap();
    assert_eq!(new_model.status, ModelStatus::Deprecated);
    assert_eq!(
        catalog.find_by_litellm_prefix("newco/").unwrap().id,
        "newco"
    );
}

#[cfg(feature = "remote-catalog")]
mod remote_tests {
    use super::*;
    use crate::catalog::helpers::unix_now_nanos;
    use crate::catalog::{cache_json_path, cache_metadata_path, RemoteCatalogOptions};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct MockResponse {
        status: &'static str,
        headers: Vec<(&'static str, &'static str)>,
        body: String,
    }

    async fn mock_server(responses: Vec<MockResponse>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let request_log = Arc::clone(&requests);
        tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut buf = vec![0; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                request_log
                    .lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&buf[..n]).to_string());
                let mut raw = format!(
                    "HTTP/1.1 {}\r\nContent-Length: {}\r\nConnection: close\r\n",
                    response.status,
                    response.body.len()
                );
                for (name, value) in response.headers {
                    raw.push_str(name);
                    raw.push_str(": ");
                    raw.push_str(value);
                    raw.push_str("\r\n");
                }
                raw.push_str("\r\n");
                raw.push_str(&response.body);
                stream.write_all(raw.as_bytes()).await.unwrap();
            }
        });
        (url, requests)
    }

    fn temp_cache_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "anyllm_providers_{name}_{}_{}",
            std::process::id(),
            unix_now_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn http_client() -> reqwest::Client {
        reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    #[tokio::test]
    async fn remote_200_writes_cache_and_304_uses_it() {
        let (url, requests) = mock_server(vec![
            MockResponse {
                status: "200 OK",
                headers: vec![("ETag", "\"abc\"")],
                body: TEST_FIXTURE.to_string(),
            },
            MockResponse {
                status: "304 Not Modified",
                headers: vec![],
                body: String::new(),
            },
        ])
        .await;
        let cache_dir = temp_cache_dir("etag");
        let options = RemoteCatalogOptions::new(url).with_cache_dir(&cache_dir);
        let client = http_client();

        let first = ProviderCatalog::fetch_litellm_with_options(&client, &options)
            .await
            .unwrap();
        assert_eq!(first.metadata().etag.as_deref(), Some("\"abc\""));
        assert!(cache_json_path(&cache_dir).exists());
        assert!(cache_metadata_path(&cache_dir).exists());

        let second = ProviderCatalog::fetch_litellm_with_options(&client, &options)
            .await
            .unwrap();
        assert!(second.get_provider("newco").is_some());
        assert!(requests.lock().unwrap()[1]
            .to_ascii_lowercase()
            .contains("if-none-match: \"abc\""));

        let _ = std::fs::remove_dir_all(cache_dir);
    }

    #[tokio::test]
    async fn remote_rejects_oversized_response() {
        let body = format!("{TEST_FIXTURE} ");
        let (url, _) = mock_server(vec![MockResponse {
            status: "200 OK",
            headers: vec![],
            body,
        }])
        .await;
        let options = RemoteCatalogOptions::new(url).with_max_bytes(8);
        let client = http_client();

        let err = ProviderCatalog::fetch_litellm_with_options(&client, &options)
            .await
            .unwrap_err();
        assert!(matches!(err, CatalogError::TooLarge { .. }));
    }

    #[tokio::test]
    async fn invalid_json_falls_back_only_when_requested() {
        let (url, _) = mock_server(vec![
            MockResponse {
                status: "200 OK",
                headers: vec![],
                body: TEST_FIXTURE.to_string(),
            },
            MockResponse {
                status: "200 OK",
                headers: vec![],
                body: "{not json".to_string(),
            },
            MockResponse {
                status: "200 OK",
                headers: vec![],
                body: "{not json".to_string(),
            },
        ])
        .await;
        let cache_dir = temp_cache_dir("invalid_json");
        let client = http_client();
        let options = RemoteCatalogOptions::new(url).with_cache_dir(&cache_dir);

        ProviderCatalog::fetch_litellm_with_options(&client, &options)
            .await
            .unwrap();
        let err = ProviderCatalog::fetch_litellm_with_options(&client, &options)
            .await
            .unwrap_err();
        assert!(matches!(err, CatalogError::Json(_)));

        let stale_options = options.with_stale_on_error(true);
        let stale = ProviderCatalog::fetch_litellm_with_options(&client, &stale_options)
            .await
            .unwrap();
        assert!(stale.get_provider("newco").is_some());

        let _ = std::fs::remove_dir_all(cache_dir);
    }
}
