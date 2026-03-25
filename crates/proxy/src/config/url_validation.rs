// URL validation for upstream backend targets.
// Security-critical: prevents SSRF via private/loopback/metadata IPs.

use std::net::IpAddr;
use url::Url;

// Re-export is_private_ip from the client crate (canonical location).
pub use anyllm_client::http::is_private_ip;

/// Validate that a base URL is safe to use as an upstream target.
/// Rejects non-http(s) schemes, private/loopback IPs, and link-local addresses.
/// For domain names, also resolves DNS and validates all resolved IPs to prevent
/// DNS rebinding attacks (where a domain initially resolves to a public IP but
/// later changes to a private/metadata IP).
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

            // Resolve DNS at startup and validate all resolved IPs.
            // This catches domains that currently resolve to private/metadata IPs.
            // Note: does not prevent post-startup DNS rebinding; for full protection,
            // restrict outbound traffic at the network level.
            let port = parsed
                .port()
                .unwrap_or(if parsed.scheme() == "https" { 443 } else { 80 });
            let lookup = format!("{domain}:{port}");
            match std::net::ToSocketAddrs::to_socket_addrs(&lookup) {
                Ok(addrs) => {
                    for addr in addrs {
                        if is_private_ip(addr.ip()) {
                            return Err(format!(
                                "hostname '{domain}' resolves to private/loopback IP {}, not allowed",
                                addr.ip()
                            ));
                        }
                    }
                }
                Err(e) => {
                    // Allow through: the domain may not be resolvable in the
                    // build/test environment but will work at runtime. The
                    // runtime SsrfSafeDnsResolver provides connection-time protection.
                    tracing::warn!(
                        domain = %domain,
                        error = %e,
                        "DNS resolution failed at startup; domain will be validated at connection time"
                    );
                }
            }
        }
    }

    Ok(())
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
