use std::net::IpAddr;
use url::Url;

/// Proxy configuration loaded from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    pub openai_api_key: String,
    pub openai_base_url: String,
    pub listen_port: u16,
}

impl Config {
    pub fn from_env() -> Self {
        let base_url = std::env::var("OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com".to_string());

        if let Err(e) = validate_base_url(&base_url) {
            panic!("OPENAI_BASE_URL rejected: {e}");
        }

        Self {
            openai_api_key: std::env::var("OPENAI_API_KEY").unwrap_or_default(),
            openai_base_url: base_url,
            listen_port: std::env::var("LISTEN_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3000),
        }
    }
}

/// Validate that a base URL is safe to use as an upstream target.
/// Rejects non-http(s) schemes, private/loopback IPs, and link-local addresses.
pub fn validate_base_url(raw: &str) -> Result<(), String> {
    let parsed = Url::parse(raw).map_err(|e| format!("invalid URL: {e}"))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => return Err(format!("scheme '{other}' not allowed, use http or https")),
    }

    match parsed.host() {
        None => return Err("URL has no host".to_string()),
        Some(url::Host::Ipv4(v4)) => {
            let ip = IpAddr::V4(v4);
            if is_private_ip(ip) {
                return Err(format!("private/loopback IP {ip} not allowed"));
            }
        }
        Some(url::Host::Ipv6(v6)) => {
            let ip = IpAddr::V6(v6);
            if is_private_ip(ip) {
                return Err(format!("private/loopback IP {ip} not allowed"));
            }
        }
        Some(url::Host::Domain(domain)) => {
            let lower = domain.to_ascii_lowercase();
            if lower == "localhost"
                || lower.ends_with(".localhost")
                || lower == "metadata.google.internal"
                || lower.ends_with(".internal")
            {
                return Err(format!("hostname '{domain}' not allowed"));
            }
        }
    }

    Ok(())
}

/// Returns true for loopback, private (RFC 1918), link-local, and
/// cloud metadata IPs (169.254.169.254).
fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                // Cloud metadata endpoint
                || v4 == std::net::Ipv4Addr::new(169, 254, 169, 254)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified()
            // ::1, ::, and IPv4-mapped private addresses
            || matches!(v6.to_ipv4_mapped(), Some(v4) if is_private_ip(IpAddr::V4(v4)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_https_url() {
        assert!(validate_base_url("https://api.openai.com").is_ok());
    }

    #[test]
    fn valid_http_url() {
        assert!(validate_base_url("http://my-proxy.example.com").is_ok());
    }

    #[test]
    fn rejects_ftp_scheme() {
        let err = validate_base_url("ftp://evil.com").unwrap_err();
        assert!(err.contains("scheme"));
    }

    #[test]
    fn rejects_localhost() {
        let err = validate_base_url("http://localhost:8080").unwrap_err();
        assert!(err.contains("not allowed"));
    }

    #[test]
    fn rejects_loopback_ip() {
        let err = validate_base_url("http://127.0.0.1:8080").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_private_10_range() {
        let err = validate_base_url("http://10.0.0.1").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_private_172_range() {
        let err = validate_base_url("http://172.16.0.1").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_private_192_range() {
        let err = validate_base_url("http://192.168.1.1").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_cloud_metadata() {
        let err = validate_base_url("http://169.254.169.254").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_metadata_hostname() {
        let err = validate_base_url("http://metadata.google.internal").unwrap_err();
        assert!(err.contains("not allowed"));
    }

    #[test]
    fn rejects_ipv6_loopback() {
        let err = validate_base_url("http://[::1]:8080").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_unspecified() {
        let err = validate_base_url("http://0.0.0.0").unwrap_err();
        assert!(err.contains("private/loopback"));
    }

    #[test]
    fn rejects_invalid_url() {
        let err = validate_base_url("not a url").unwrap_err();
        assert!(err.contains("invalid URL"));
    }
}
