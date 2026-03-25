//! HTTP client builder with optional mTLS, custom CA, and SSRF-safe DNS resolution.

use reqwest::Client;
use std::net::IpAddr;
use std::time::Duration;

/// Configuration for building an HTTP client.
#[derive(Clone, Debug, Default)]
pub struct HttpClientConfig {
    /// PKCS#12 identity bytes and password for mTLS.
    pub p12_identity: Option<(Vec<u8>, String)>,
    /// PEM-encoded CA certificate for verifying the backend server.
    pub ca_cert_pem: Option<Vec<u8>>,
    /// Connection timeout (default: 10s).
    pub connect_timeout: Option<Duration>,
    /// Read timeout (default: 900s, generous for reasoning models).
    pub read_timeout: Option<Duration>,
    /// TCP keepalive interval (default: 60s).
    pub tcp_keepalive: Option<Duration>,
    /// Enable SSRF-safe DNS resolution (default: true when `ssrf-protection` feature enabled).
    pub ssrf_protection: bool,
}

impl HttpClientConfig {
    pub fn new() -> Self {
        Self {
            ssrf_protection: cfg!(feature = "ssrf-protection"),
            ..Default::default()
        }
    }
}

/// Build a reqwest HTTP client from configuration.
///
/// Includes hardened defaults: 10s connect timeout, 900s read timeout (for slow
/// reasoning models like o1/o3), 60s TCP keepalive, and SSRF-safe DNS resolution.
pub fn build_http_client(config: &HttpClientConfig) -> Client {
    let mut builder = Client::builder();

    if let Some((ref p12_bytes, ref password)) = config.p12_identity {
        let identity = reqwest::Identity::from_pkcs12_der(p12_bytes, password)
            .expect("P12 identity was validated at startup");
        builder = builder.identity(identity);
    }

    if let Some(ref ca_pem) = config.ca_cert_pem {
        let cert =
            reqwest::Certificate::from_pem(ca_pem).expect("CA cert was validated at startup");
        builder = builder.add_root_certificate(cert);
    }

    let connect_timeout = config.connect_timeout.unwrap_or(Duration::from_secs(10));
    let read_timeout = config.read_timeout.unwrap_or(Duration::from_secs(900));
    let tcp_keepalive = config.tcp_keepalive.unwrap_or(Duration::from_secs(60));

    builder = builder
        .connect_timeout(connect_timeout)
        .read_timeout(read_timeout)
        .tcp_keepalive(tcp_keepalive);

    #[cfg(feature = "ssrf-protection")]
    if config.ssrf_protection {
        builder = builder.dns_resolver(std::sync::Arc::new(SsrfSafeDnsResolver));
    }

    builder.build().expect("failed to build HTTP client")
}

/// DNS resolver that rejects private/loopback IPs at connection time,
/// preventing DNS rebinding attacks where a domain resolves to a public IP
/// at startup validation but later resolves to a private/metadata IP.
#[cfg(feature = "ssrf-protection")]
struct SsrfSafeDnsResolver;

#[cfg(feature = "ssrf-protection")]
impl reqwest::dns::Resolve for SsrfSafeDnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        Box::pin(async move {
            let name_str = name.as_str().to_string();
            // DNS resolution (ToSocketAddrs) blocks the calling thread.
            // Must run on the blocking threadpool to avoid stalling the
            // async runtime and all other in-flight requests.
            let addrs: Vec<std::net::SocketAddr> =
                tokio::task::spawn_blocking(move || -> Result<Vec<std::net::SocketAddr>, _> {
                    use std::net::ToSocketAddrs;
                    // Port 0 is a placeholder; reqwest replaces it with the actual port.
                    let lookup = format!("{name_str}:0");
                    Ok(lookup.to_socket_addrs()?.collect())
                })
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) })?
                .map_err(
                    |e: std::io::Error| -> Box<dyn std::error::Error + Send + Sync> { Box::new(e) },
                )?;

            // Filter out private/loopback IPs to prevent SSRF attacks where
            // an attacker-controlled DNS record resolves to internal endpoints
            // (e.g., cloud metadata at 169.254.169.254).
            let safe: Vec<std::net::SocketAddr> = addrs
                .into_iter()
                .filter(|addr| !is_private_ip(addr.ip()))
                .collect();

            if safe.is_empty() {
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "DNS resolved only to private/loopback IPs (SSRF blocked)".to_string(),
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }

            Ok(Box::new(safe.into_iter()) as Box<dyn Iterator<Item = std::net::SocketAddr> + Send>)
        })
    }
}

/// Returns true for loopback, private (RFC 1918), link-local, and
/// cloud metadata IPs (169.254.169.254).
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                // AWS/GCP/Azure metadata endpoint. SSRF to this IP lets
                // attackers exfiltrate instance credentials.
                || v4 == std::net::Ipv4Addr::new(169, 254, 169, 254)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified()
            // Check IPv4-mapped IPv6 addresses (::ffff:192.168.x.x) recursively;
            // attackers can bypass IPv4 checks using the mapped representation.
            || matches!(v6.to_ipv4_mapped(), Some(v4) if is_private_ip(IpAddr::V4(v4)))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_ipv4_loopback() {
        assert!(is_private_ip("127.0.0.1".parse().unwrap()));
    }

    #[test]
    fn private_ipv4_rfc1918() {
        assert!(is_private_ip("10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("172.16.0.1".parse().unwrap()));
        assert!(is_private_ip("192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn private_ipv4_link_local() {
        assert!(is_private_ip("169.254.1.1".parse().unwrap()));
    }

    #[test]
    fn private_ipv4_metadata() {
        assert!(is_private_ip("169.254.169.254".parse().unwrap()));
    }

    #[test]
    fn private_ipv4_unspecified() {
        assert!(is_private_ip("0.0.0.0".parse().unwrap()));
    }

    #[test]
    fn public_ipv4() {
        assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn private_ipv6_loopback() {
        assert!(is_private_ip("::1".parse().unwrap()));
    }

    #[test]
    fn private_ipv6_mapped_private() {
        // ::ffff:192.168.1.1
        assert!(is_private_ip("::ffff:192.168.1.1".parse().unwrap()));
    }

    #[test]
    fn public_ipv6() {
        assert!(!is_private_ip("2001:4860:4860::8888".parse().unwrap()));
    }

    #[test]
    fn default_config_has_ssrf_protection() {
        let config = HttpClientConfig::new();
        assert_eq!(config.ssrf_protection, cfg!(feature = "ssrf-protection"));
    }

    #[test]
    fn build_client_default_config() {
        let config = HttpClientConfig {
            ssrf_protection: false, // avoid DNS in tests
            ..Default::default()
        };
        let _client = build_http_client(&config);
    }
}
