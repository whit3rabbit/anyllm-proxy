use anyllm_proxy::{config::Config, server::routes};
use tokio::net::TcpListener;

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let config = Config::from_env();
    let app = routes::app(config);

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    let resp = reqwest::get(format!("http://{addr}/health")).await.unwrap();

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body, serde_json::json!({ "status": "ok" }));
}
