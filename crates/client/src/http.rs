//! HTTP client builder with optional mTLS, custom CA, and SSRF-safe DNS resolution.

use reqwest::Client;
use std::net::IpAddr;
use std::time::Duration;

/// Configuration for building an HTTP client.
#[derive(Clone, Default)]
pub struct HttpClientConfig {
    /// PKCS#12 identity bytes and password for mTLS.
    /// Password wrapped in Zeroizing to clear from heap on drop.
    pub p12_identity: Option<(Vec<u8>, zeroize::Zeroizing<String>)>,
    /// PEM-encoded CA certificate for verifying the backend server.
    pub ca_cert_pem: Option<Vec<u8>>,
    /// Connection timeout (default: 10s).
    pub connect_timeout: Option<Duration>,
    /// Total request timeout — wall-clock limit from first byte sent to last byte received.
    /// Unset by default; read_timeout already caps slow responses.
    pub request_timeout: Option<Duration>,
    /// Read timeout (default: 900s, generous for reasoning models).
    pub read_timeout: Option<Duration>,
    /// TCP keepalive interval (default: 60s).
    pub tcp_keepalive: Option<Duration>,
    /// Enable SSRF-safe DNS resolution (default: true when `ssrf-protection` feature enabled).
    pub ssrf_protection: bool,
    /// Allow requests to loopback addresses (127.x, ::1) when SSRF protection is on.
    ///
    /// Useful for local development backends (e.g. Ollama on localhost:11434).
    /// Note: literal-IP URLs (`http://127.0.0.1:8080`) bypass DNS and are always
    /// reachable regardless of this flag — only hostname lookups (e.g. `localhost`)
    /// are filtered by the SSRF resolver.
    pub ssrf_allow_loopback: bool,
    /// Allow requests to RFC 1918 private addresses when SSRF protection is on.
    ///
    /// Useful for backend servers on a private network. Cloud metadata endpoints
    /// (169.254.169.254, link-local) remain blocked regardless of this flag.
    pub ssrf_allow_private: bool,
    /// Static headers sent on every request (e.g. OpenRouter `HTTP-Referer`/`X-Title`).
    ///
    /// Applied as `default_headers` on the reqwest client. Invalid header names or values
    /// are skipped with a warning rather than panicking.
    ///
    /// Note: [`crate::client::Client::with_http_client`] callers own their reqwest client
    /// and must set default headers themselves; this field is applied only by
    /// [`build_http_client`].
    pub extra_headers: Vec<(String, String)>,
}

impl std::fmt::Debug for HttpClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Header values are redacted — they may contain gateway tokens.
        let header_names: Vec<&str> = self.extra_headers.iter().map(|(k, _)| k.as_str()).collect();
        f.debug_struct("HttpClientConfig")
            .field(
                "p12_identity",
                &self.p12_identity.as_ref().map(|_| "[REDACTED]"),
            )
            .field(
                "ca_cert_pem",
                &self
                    .ca_cert_pem
                    .as_ref()
                    .map(|b| format!("{} bytes", b.len())),
            )
            .field("connect_timeout", &self.connect_timeout)
            .field("request_timeout", &self.request_timeout)
            .field("read_timeout", &self.read_timeout)
            .field("tcp_keepalive", &self.tcp_keepalive)
            .field("ssrf_protection", &self.ssrf_protection)
            .field("ssrf_allow_loopback", &self.ssrf_allow_loopback)
            .field("ssrf_allow_private", &self.ssrf_allow_private)
            .field("extra_headers (names only)", &header_names)
            .finish()
    }
}

impl HttpClientConfig {
    /// Create an `HttpClientConfig` with SSRF protection enabled when the
    /// `ssrf-protection` feature is active, and all other options at default.
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
        #[cfg(feature = "native-tls")]
        {
            let identity = reqwest::Identity::from_pkcs12_der(p12_bytes, password)
                .expect("P12 identity was validated at startup");
            builder = builder.identity(identity);
        }
        #[cfg(not(feature = "native-tls"))]
        {
            let _ = (p12_bytes, password); // suppress unused warnings
            panic!(
                "p12_identity requires the native-tls feature; \
                 enable it or use a PEM identity instead"
            );
        }
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

    if let Some(rt) = config.request_timeout {
        builder = builder.timeout(rt);
    }

    if !config.extra_headers.is_empty() {
        let mut header_map = reqwest::header::HeaderMap::new();
        for (name, value) in &config.extra_headers {
            match (
                reqwest::header::HeaderName::from_bytes(name.as_bytes()),
                reqwest::header::HeaderValue::from_str(value),
            ) {
                (Ok(hn), Ok(hv)) => {
                    if header_map.insert(hn, hv).is_some() {
                        tracing::warn!(
                            name = name.as_str(),
                            "extra_header overwrites a previous value for the same name"
                        );
                    }
                }
                _ => {
                    tracing::warn!(
                        name = name.as_str(),
                        "skipping extra_header with invalid name or value"
                    );
                }
            }
        }
        builder = builder.default_headers(header_map);
    }

    #[cfg(feature = "ssrf-protection")]
    if config.ssrf_protection {
        // Disable redirects in addition to DNS filtering. The DNS resolver only
        // intercepts hostname lookups; a redirect to a bare IP (e.g.,
        // http://169.254.169.254/) bypasses DNS entirely, so the SSRF-safe
        // resolver would never be called and the redirect would be followed.
        builder = builder
            .dns_resolver(std::sync::Arc::new(SsrfSafeDnsResolver {
                allow_loopback: config.ssrf_allow_loopback,
                allow_private: config.ssrf_allow_private,
            }))
            .redirect(reqwest::redirect::Policy::none());
    }

    builder.build().expect("failed to build HTTP client")
}

/// DNS resolver that rejects private/loopback IPs at connection time,
/// preventing DNS rebinding attacks where a domain resolves to a public IP
/// at startup validation but later resolves to a private/metadata IP.
#[cfg(feature = "ssrf-protection")]
struct SsrfSafeDnsResolver {
    allow_loopback: bool,
    allow_private: bool,
}

#[cfg(feature = "ssrf-protection")]
impl reqwest::dns::Resolve for SsrfSafeDnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let allow_loopback = self.allow_loopback;
        let allow_private = self.allow_private;
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

            // Filter out blocked IPs to prevent SSRF attacks where
            // an attacker-controlled DNS record resolves to internal endpoints
            // (e.g., cloud metadata at 169.254.169.254).
            let safe: Vec<std::net::SocketAddr> = addrs
                .into_iter()
                .filter(|addr| !is_blocked_ip(addr.ip(), allow_loopback, allow_private))
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

/// Returns true if `ip` should be blocked by SSRF protection given the
/// supplied permission flags.
///
/// Rules (regardless of flags):
/// - Link-local (169.254.x.x, fe80::/10) and cloud metadata (169.254.169.254)
///   are **always** blocked. `allow_private` does **not** open these.
/// - Unspecified (`0.0.0.0`, `::`) and broadcast are always blocked.
/// - RFC 6666 discard prefix (100::/64) and RFC 3849 documentation
///   (2001:db8::/32) are always blocked.
///
/// Flags:
/// - `allow_loopback`: when `false`, blocks 127.x.x.x / `::1` / mapped loopback.
/// - `allow_private`: when `false`, blocks RFC 1918 (10/8, 172.16/12, 192.168/16)
///   and IPv6 ULA (fc00::/7).
pub fn is_blocked_ip(ip: IpAddr, allow_loopback: bool, allow_private: bool) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            // Always-blocked regardless of flags.
            if v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4 == std::net::Ipv4Addr::new(169, 254, 169, 254)
            {
                return true;
            }
            if v4.is_loopback() && !allow_loopback {
                return true;
            }
            if v4.is_private() && !allow_private {
                return true;
            }
            false
        }
        IpAddr::V6(v6) => {
            let seg0 = v6.segments()[0];
            let seg1 = v6.segments()[1];

            // Always-blocked regardless of flags.
            if v6.is_unspecified()
                // Link-Local (fe80::/10)
                || (seg0 & 0xffc0) == 0xfe80
                // Discard (RFC 6666, 100::/64)
                || (seg0 == 0x0100
                    && seg1 == 0x0000
                    && v6.segments()[2] == 0
                    && v6.segments()[3] == 0)
                // Documentation (RFC 3849, 2001:db8::/32)
                || (seg0 == 0x2001 && seg1 == 0x0db8)
            {
                return true;
            }

            // IPv4-mapped: check recursively against IPv4 rules so that
            // ::ffff:192.168.1.1 respects the same flags as 192.168.1.1.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_blocked_ip(IpAddr::V4(v4), allow_loopback, allow_private);
            }

            if v6.is_loopback() && !allow_loopback {
                return true;
            }
            // IPv6 ULA (fc00::/7): covers fc00:: through fdff::
            if (seg0 & 0xfe00) == 0xfc00 && !allow_private {
                return true;
            }
            false
        }
    }
}

/// Returns true for loopback, private (RFC 1918), link-local, and
/// cloud metadata IPs (169.254.169.254).
///
/// Delegates to [`is_blocked_ip`] with both flags off (original strict behavior).
pub fn is_private_ip(ip: IpAddr) -> bool {
    is_blocked_ip(ip, false, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_private_ip (backward-compat) ---

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
    fn private_ipv6_ula() {
        // fc00::/7 covers fc00:: through fdff::
        assert!(is_private_ip("fc00::1".parse().unwrap()));
        assert!(is_private_ip("fd12:3456:789a:1::1".parse().unwrap()));
        assert!(is_private_ip("fdff:ffff:ffff:ffff::1".parse().unwrap()));
    }

    #[test]
    fn private_ipv6_link_local() {
        // fe80::/10 covers fe80:: through febf::
        assert!(is_private_ip("fe80::1".parse().unwrap()));
        assert!(is_private_ip("fe80::dead:beef".parse().unwrap()));
        assert!(is_private_ip("febf::1".parse().unwrap()));
    }

    #[test]
    fn private_ipv6_discard_prefix() {
        // RFC 6666: 100::/64
        assert!(is_private_ip("100::".parse().unwrap()));
        assert!(is_private_ip("100::1".parse().unwrap()));
        // Outside /64 should not match
        assert!(!is_private_ip("100:0:0:1::".parse().unwrap()));
    }

    #[test]
    fn private_ipv6_documentation() {
        // RFC 3849: 2001:db8::/32
        assert!(is_private_ip("2001:db8::1".parse().unwrap()));
        assert!(is_private_ip("2001:db8:1234:5678::1".parse().unwrap()));
        // Outside /32 should not match
        assert!(!is_private_ip("2001:db9::1".parse().unwrap()));
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

    // --- is_blocked_ip granular flag tests ---

    #[test]
    fn loopback_blocked_by_default() {
        assert!(is_blocked_ip("127.0.0.1".parse().unwrap(), false, false));
    }

    #[test]
    fn loopback_allowed_when_flag_set() {
        assert!(!is_blocked_ip("127.0.0.1".parse().unwrap(), true, false));
    }

    #[test]
    fn ipv6_loopback_allowed_when_flag_set() {
        assert!(!is_blocked_ip("::1".parse().unwrap(), true, false));
    }

    #[test]
    fn rfc1918_blocked_by_default() {
        assert!(is_blocked_ip("192.168.1.1".parse().unwrap(), false, false));
    }

    #[test]
    fn rfc1918_allowed_when_private_flag_set() {
        assert!(!is_blocked_ip("192.168.1.1".parse().unwrap(), false, true));
    }

    #[test]
    fn ipv6_ula_allowed_when_private_flag_set() {
        assert!(!is_blocked_ip("fd12::1".parse().unwrap(), false, true));
    }

    #[test]
    fn metadata_always_blocked_regardless_of_flags() {
        // allow_loopback and allow_private both true — metadata must still be blocked.
        assert!(is_blocked_ip(
            "169.254.169.254".parse().unwrap(),
            true,
            true
        ));
    }

    #[test]
    fn link_local_always_blocked_regardless_of_flags() {
        assert!(is_blocked_ip("169.254.1.1".parse().unwrap(), true, true));
        assert!(is_blocked_ip("fe80::1".parse().unwrap(), true, true));
    }

    #[test]
    fn equivalence_with_is_private_ip() {
        let ips: Vec<IpAddr> = vec![
            "127.0.0.1".parse().unwrap(),
            "10.0.0.1".parse().unwrap(),
            "192.168.1.1".parse().unwrap(),
            "169.254.169.254".parse().unwrap(),
            "8.8.8.8".parse().unwrap(),
            "::1".parse().unwrap(),
            "fc00::1".parse().unwrap(),
            "fe80::1".parse().unwrap(),
            "2001:4860:4860::8888".parse().unwrap(),
        ];
        for ip in ips {
            assert_eq!(
                is_private_ip(ip),
                is_blocked_ip(ip, false, false),
                "mismatch for {ip}"
            );
        }
    }

    // --- extra_headers debug redaction ---

    #[test]
    fn debug_redacts_header_values() {
        let config = HttpClientConfig {
            extra_headers: vec![
                ("HTTP-Referer".into(), "https://myapp.com".into()),
                ("X-Title".into(), "My App".into()),
            ],
            ssrf_protection: false,
            ..Default::default()
        };
        let debug_str = format!("{config:?}");
        // Names visible, values not
        assert!(debug_str.contains("HTTP-Referer"));
        assert!(debug_str.contains("X-Title"));
        assert!(!debug_str.contains("https://myapp.com"));
        assert!(!debug_str.contains("My App"));
    }

    #[test]
    fn invalid_header_name_does_not_panic() {
        let config = HttpClientConfig {
            extra_headers: vec![
                ("valid-header".into(), "value".into()),
                ("invalid header with spaces".into(), "value".into()),
            ],
            ssrf_protection: false,
            ..Default::default()
        };
        // Must not panic; the invalid entry is skipped with a warning.
        let _client = build_http_client(&config);
    }

    #[cfg(not(feature = "native-tls"))]
    #[test]
    #[should_panic(expected = "p12_identity requires the native-tls feature")]
    fn p12_identity_panics_without_native_tls() {
        let config = HttpClientConfig {
            p12_identity: Some((vec![0u8], zeroize::Zeroizing::new("pw".into()))),
            ssrf_protection: false,
            ..Default::default()
        };
        build_http_client(&config);
    }
}
