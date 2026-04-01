// Optional mTLS configuration for the backend connection.

use std::fmt;

/// Optional mTLS configuration for the backend connection.
/// Stores raw certificate bytes so Config remains Clone.
/// Validated at construction time: bad certs cause startup panic.
/// Password wrapped in Zeroizing so it is zeroed from heap on drop.
#[derive(Clone, Default)]
pub struct TlsConfig {
    /// Raw PKCS#12 bytes and password for client certificate authentication.
    pub p12_identity: Option<(Vec<u8>, zeroize::Zeroizing<String>)>,
    /// Raw PEM bytes for additional CA certificate to trust.
    pub ca_cert_pem: Option<Vec<u8>>,
}

impl fmt::Debug for TlsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConfig")
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
            .finish()
    }
}

impl TlsConfig {
    /// Load and validate TLS config from file paths.
    /// Panics on invalid/missing files or wrong password.
    pub fn load(p12_path: Option<&str>, p12_password: Option<&str>, ca_path: Option<&str>) -> Self {
        let p12_identity = match (p12_path, p12_password) {
            (Some(path), Some(password)) => {
                let bytes = std::fs::read(path)
                    .unwrap_or_else(|e| panic!("failed to read P12 file '{}': {}", path, e));

                // Validate the P12 parses correctly with the given password
                reqwest::Identity::from_pkcs12_der(&bytes, password).unwrap_or_else(|e| {
                    panic!(
                        "invalid P12 file '{}' (wrong password or corrupt file): {}",
                        path, e
                    )
                });

                tracing::info!(path = %path, "loaded client certificate (P12)");
                Some((bytes, zeroize::Zeroizing::new(password.to_string())))
            }
            (Some(_), None) => {
                panic!("TLS_CLIENT_CERT_P12 is set but TLS_CLIENT_CERT_PASSWORD is missing");
            }
            (None, Some(_)) => {
                tracing::warn!(
                    "TLS_CLIENT_CERT_PASSWORD is set but TLS_CLIENT_CERT_P12 is not, ignoring"
                );
                None
            }
            (None, None) => None,
        };

        let ca_cert_pem = ca_path.map(|path| {
            let bytes = std::fs::read(path)
                .unwrap_or_else(|e| panic!("failed to read CA cert file '{}': {}", path, e));

            // Validate the PEM parses as a certificate
            reqwest::Certificate::from_pem(&bytes)
                .unwrap_or_else(|e| panic!("invalid CA certificate '{}': {}", path, e));

            tracing::info!(path = %path, "loaded custom CA certificate");
            bytes
        });

        Self {
            p12_identity,
            ca_cert_pem,
        }
    }

    /// Load from environment variables.
    pub fn from_env() -> Self {
        let p12_path = std::env::var("TLS_CLIENT_CERT_P12").ok();
        let p12_password = std::env::var("TLS_CLIENT_CERT_PASSWORD").ok();
        let ca_path = std::env::var("TLS_CA_CERT").ok();
        Self::load(
            p12_path.as_deref(),
            p12_password.as_deref(),
            ca_path.as_deref(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Path to test fixtures relative to the workspace root.
    fn fixture_path(name: &str) -> String {
        let manifest = env!("CARGO_MANIFEST_DIR");
        format!("{manifest}/tests/fixtures/tls/{name}")
    }

    #[test]
    fn tls_config_none_when_no_paths() {
        let tls = TlsConfig::load(None, None, None);
        assert!(tls.p12_identity.is_none());
        assert!(tls.ca_cert_pem.is_none());
    }

    #[test]
    #[should_panic(expected = "TLS_CLIENT_CERT_PASSWORD is missing")]
    fn tls_config_panics_missing_password() {
        TlsConfig::load(Some("/any/path.p12"), None, None);
    }

    #[test]
    #[should_panic(expected = "failed to read P12 file")]
    fn tls_config_panics_missing_p12_file() {
        TlsConfig::load(Some("/nonexistent/file.p12"), Some("pass"), None);
    }

    #[test]
    fn tls_config_loads_valid_p12() {
        let path = fixture_path("test-client.p12");
        let tls = TlsConfig::load(Some(&path), Some("test"), None);
        assert!(tls.p12_identity.is_some());
        assert!(tls.ca_cert_pem.is_none());
    }

    #[test]
    fn tls_config_loads_valid_ca() {
        let path = fixture_path("test-ca.pem");
        let tls = TlsConfig::load(None, None, Some(&path));
        assert!(tls.p12_identity.is_none());
        assert!(tls.ca_cert_pem.is_some());
    }

    #[test]
    fn tls_config_loads_both() {
        let p12 = fixture_path("test-client.p12");
        let ca = fixture_path("test-ca.pem");
        let tls = TlsConfig::load(Some(&p12), Some("test"), Some(&ca));
        assert!(tls.p12_identity.is_some());
        assert!(tls.ca_cert_pem.is_some());
    }

    #[test]
    fn tls_config_debug_redacts_password() {
        let p12 = fixture_path("test-client.p12");
        let tls = TlsConfig::load(Some(&p12), Some("test"), None);
        let debug = format!("{:?}", tls);
        assert!(debug.contains("REDACTED"));
        assert!(!debug.contains("test"));
    }

    #[test]
    #[should_panic(expected = "invalid P12 file")]
    fn tls_config_panics_wrong_password() {
        let path = fixture_path("test-client.p12");
        TlsConfig::load(Some(&path), Some("wrong-password"), None);
    }
}
