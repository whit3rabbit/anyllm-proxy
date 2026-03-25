use anyllm_proxy::config::{self, Config};
use anyllm_proxy::server::routes;

fn test_config_with_logging() -> Config {
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
        log_bodies: true,
        openai_api_format: config::OpenAIApiFormat::Chat,
    }
}

#[tokio::test]
async fn server_starts_with_body_logging_enabled() {
    let app = routes::app(test_config_with_logging());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();
    assert_eq!(resp.status(), 200);
}
