pub mod anthropic_headers;
pub mod auth;
pub mod ip_allowlist;
pub mod request_id;

pub use anthropic_headers::log_anthropic_headers;
pub use auth::{
    set_hmac_secret, set_oidc_config, set_virtual_keys, validate_auth, AuthMode, VirtualKeyContext,
};
pub use ip_allowlist::{check_ip_allowlist, ip_allowlist_active, is_ip_allowed};
pub use request_id::add_request_id;

/// Maximum request body size (32 MB, matching Anthropic's Messages endpoint limit).
pub const MAX_BODY_SIZE: usize = 32 * 1024 * 1024;

/// Maximum concurrent requests to prevent self-DOS under 429 incidents.
pub const MAX_CONCURRENT_REQUESTS: usize = 100;
