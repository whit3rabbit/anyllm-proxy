use std::sync::Arc;

pub(crate) async fn init_oidc() {
    // OIDC/JWT authentication (optional). When OIDC_ISSUER_URL is set, discover
    // the OIDC configuration and load JWKS. Tokens that look like JWTs are
    // validated against the JWKS before falling through to key-based auth.
    if let Ok(issuer_url) = std::env::var("OIDC_ISSUER_URL") {
        let audience = std::env::var("OIDC_AUDIENCE").unwrap_or_else(|_| {
            tracing::warn!(
                "OIDC_ISSUER_URL is set but OIDC_AUDIENCE is not; using issuer URL as audience"
            );
            issuer_url.clone()
        });
        match anyllm_proxy::server::oidc::OidcConfig::discover(&issuer_url, &audience).await {
            Ok(config) => {
                let config = Arc::new(config);
                anyllm_proxy::server::middleware::set_oidc_config(config.clone());
                // Background task: refresh JWKS every 60 minutes.
                tokio::spawn(async move {
                    let mut interval = tokio::time::interval(std::time::Duration::from_secs(3600));
                    // NOT A BUG: tokio::time::interval fires immediately on creation.
                    // Consuming the first tick here skips that immediate fire so the
                    // first actual refresh happens after 60 minutes, not at startup.
                    interval.tick().await; // consume immediate first tick
                    loop {
                        interval.tick().await; // wait 60 minutes between refreshes
                        if let Err(e) = config.refresh_jwks().await {
                            tracing::warn!("JWKS refresh failed: {e}");
                        } else {
                            tracing::debug!("JWKS refreshed successfully");
                        }
                    }
                });
                tracing::info!(issuer = %issuer_url, "OIDC/JWT authentication enabled");
            }
            Err(e) => {
                tracing::error!("OIDC discovery failed: {e}. Starting without OIDC auth.");
            }
        }
    }
}
