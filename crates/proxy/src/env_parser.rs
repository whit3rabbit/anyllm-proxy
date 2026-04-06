//! Pure `.anyllm.env`-format parser. No I/O, no `set_var` — safe from any context including tests.
//!
//! Format rules (bash-compatible subset):
//! - Lines starting with `#` are comments.
//! - `KEY=value` or `KEY="value"` or `KEY='value'`.
//! - Double-quoted values interpret `\n`, `\t`, `\r`, `\\`, `\"`.
//! - Single-quoted values are literal (no escape processing, matching bash behavior).
//! - Keys must match `[A-Z_][A-Z0-9_]*` (POSIX portable env var names).

use serde::Serialize;
use std::collections::HashSet;
use std::sync::LazyLock;

/// All known env var names accepted by anyllm_proxy.
/// Import warns on keys not in this list but still applies them.
pub(crate) const KNOWN_KEYS: &[&str] = &[
    // Core proxy
    "BACKEND",
    "LISTEN_PORT",
    "BIG_MODEL",
    "SMALL_MODEL",
    "RUST_LOG",
    "LOG_BODIES",
    "PROXY_CONFIG",
    "REQUEST_TIMEOUT_SECS",
    "MODEL_PRICING_FILE",
    "ANYLLM_DEGRADATION_WARNINGS",
    // OpenAI / compatible
    "OPENAI_BASE_URL",
    "OPENAI_API_FORMAT",
    "OPENAI_API_KEY",
    // Vertex AI
    "VERTEX_PROJECT",
    "VERTEX_REGION",
    "VERTEX_API_KEY",
    "GOOGLE_ACCESS_TOKEN",
    // Gemini
    "GEMINI_BASE_URL",
    "GEMINI_API_KEY",
    // Azure OpenAI
    "AZURE_OPENAI_ENDPOINT",
    "AZURE_OPENAI_DEPLOYMENT",
    "AZURE_OPENAI_API_KEY",
    "AZURE_OPENAI_API_VERSION",
    // AWS Bedrock
    "AWS_REGION",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    // Auth / relay
    "PROXY_API_KEYS",
    "PROXY_OPEN_RELAY",
    // TLS
    "TLS_CLIENT_CERT_P12",
    "TLS_CLIENT_CERT_PASSWORD",
    "TLS_CA_CERT",
    // Network / security
    "IP_ALLOWLIST",
    "TRUST_PROXY_HEADERS",
    "WEBHOOK_URLS",
    "RATE_LIMIT_FAIL_POLICY",
    // Admin
    "ADMIN_PORT",
    "ADMIN_BIND",
    "ADMIN_DB_PATH",
    "ADMIN_TOKEN_PATH",
    "ADMIN_TOKEN",
    "DISABLE_ADMIN",
    "WEBUI",
    "ADMIN_LOG_RETENTION_DAYS",
    // OIDC / JWT
    "OIDC_ISSUER_URL",
    "OIDC_AUDIENCE",
    // Optional backends
    "REDIS_URL",
    "QDRANT_URL",
    "QDRANT_COLLECTION",
    // OpenTelemetry
    "OTEL_EXPORTER_OTLP_ENDPOINT",
    "OTEL_SERVICE_NAME",
    "OTEL_TRACES_SAMPLER",
    // Langfuse
    "LANGFUSE_PUBLIC_KEY",
    "LANGFUSE_SECRET_KEY",
    "LANGFUSE_HOST",
    // LiteLLM aliases
    "LITELLM_MASTER_KEY",
    "LITELLM_CONFIG",
    "AZURE_API_KEY",
    "AZURE_API_BASE",
    "AZURE_API_VERSION",
    "AWS_REGION_NAME",
    "LITELLM_IP_ALLOWLIST",
];

/// Keys that warrant an extra warning when overwritten via import.
const SENSITIVE_KEYS: &[&str] = &["ADMIN_TOKEN", "ADMIN_TOKEN_PATH"];

/// Pre-built set of known keys for O(1) lookup during parsing.
static KNOWN_KEYS_SET: LazyLock<HashSet<&'static str>> =
    LazyLock::new(|| KNOWN_KEYS.iter().copied().collect());

/// A successfully parsed key-value pair from an env file.
#[derive(Debug, Clone, Serialize)]
pub struct ParsedPair {
    pub key: String,
    pub value: String,
    /// 1-based source line number (useful for error reporting).
    pub line: usize,
}

/// A non-fatal issue found during parsing (unknown key, duplicate, sensitive overwrite, etc.).
#[derive(Debug, Clone, Serialize)]
pub struct EnvWarning {
    /// Source line number (1-based), if applicable.
    pub line: Option<usize>,
    /// The env key associated with the warning, if applicable.
    pub key: Option<String>,
    pub message: String,
}

/// The result of parsing a `.anyllm.env` file. If `hard_errors` is non-empty
/// the caller must abort — do NOT write `pairs` to the database.
#[derive(Debug)]
pub struct ParseResult {
    /// Valid key-value entries extracted from the file.
    pub pairs: Vec<ParsedPair>,
    /// Non-fatal issues (applied, but surfaced to the user).
    pub warnings: Vec<EnvWarning>,
    /// Fatal errors — caller must abort and NOT write to DB.
    pub hard_errors: Vec<String>,
}

/// Interpret backslash escapes in double-quoted dotenv values.
/// Handles: `\n`, `\t`, `\r`, `\\`, `\"`.
/// Other sequences pass through unchanged.
fn unescape_double_quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Escape a value for double-quoted `.anyllm.env` output.
/// Produces a string safe to wrap in `"..."`.
pub fn escape_for_env_file(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// Parse `.anyllm.env`-format content without any I/O or side effects.
///
/// Hard errors in the result mean the caller must NOT apply the pairs.
/// Warnings are informational — the pairs are still valid and can be applied.
pub fn parse_env_content(content: &str) -> ParseResult {
    let mut pairs: Vec<ParsedPair> = Vec::new();
    let mut warnings: Vec<EnvWarning> = Vec::new();
    let mut hard_errors: Vec<String> = Vec::new();
    let mut seen_keys: HashSet<String> = HashSet::new();

    // Hard reject: null bytes indicate binary content (e.g., a PKCS#12 file uploaded
    // by mistake). Fast-fail here before doing any line-by-line parsing to avoid
    // producing confusing "key not found" errors for binary data.
    if content.contains('\0') {
        hard_errors.push("file contains binary content (null bytes)".to_string());
        return ParseResult {
            pairs,
            warnings,
            hard_errors,
        };
    }

    let mut had_content = false; // tracks whether any non-comment, non-blank lines exist

    for (idx, raw) in content.lines().enumerate() {
        let lineno = idx + 1;
        let line = raw.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        had_content = true;

        // Enforce per-line length limit.
        if line.len() > 4096 {
            warnings.push(EnvWarning {
                line: Some(lineno),
                key: None,
                message: format!("line {} exceeds 4096 characters, skipping", lineno),
            });
            continue;
        }

        // Strip optional `export ` prefix.
        let line = line
            .strip_prefix("export ")
            .map(str::trim)
            .unwrap_or(line);

        // Split on first `=`.
        let Some((raw_key, val)) = line.split_once('=') else {
            warnings.push(EnvWarning {
                line: Some(lineno),
                key: None,
                message: format!("line {} has no '=', skipping", lineno),
            });
            continue;
        };

        let key = raw_key.trim();
        if key.is_empty() {
            continue;
        }

        // Hard reject: key must match [A-Z_][A-Z0-9_]* (POSIX shell variable rule).
        // This prevents injection via malformed key names.
        if !is_valid_key(key) {
            hard_errors.push(format!(
                "line {lineno}: key {key:?} is not a valid env var name \
                 (must match [A-Z_][A-Z0-9_]*)"
            ));
            // Collect all errors rather than returning on first one.
            continue;
        }

        // Parse value (quotes + escape handling).
        // Three modes, matching bash dotenv semantics:
        //  - Double-quoted: strip outer `"`, apply backslash escapes (\n, \t, \\, \").
        //  - Single-quoted: strip outer `'`, take literal (bash single-quote = no escapes).
        //  - Unquoted: take as-is after trim.
        // `len() >= 2` guards against the degenerate input `"` (a single quote char)
        // which would produce an empty body slice and an incorrect value.
        let val = val.trim();
        let value: String = if val.starts_with('"') && val.ends_with('"') && val.len() >= 2 {
            unescape_double_quoted(&val[1..val.len() - 1])
        } else if val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2 {
            // Single-quoted values are literal in bash; no escape processing.
            val[1..val.len() - 1].to_string()
        } else {
            val.to_string()
        };

        // Warn: empty value.
        if value.is_empty() {
            warnings.push(EnvWarning {
                line: Some(lineno),
                key: Some(key.to_string()),
                message: format!("key {key:?} has an empty value"),
            });
        }

        // Warn: value suspiciously long.
        if value.len() > 2048 {
            warnings.push(EnvWarning {
                line: Some(lineno),
                key: Some(key.to_string()),
                message: format!(
                    "key {key:?} value is {} chars (> 2048), verify it is correct",
                    value.len()
                ),
            });
        }

        // Warn: unknown key.
        if !KNOWN_KEYS_SET.contains(key) {
            warnings.push(EnvWarning {
                line: Some(lineno),
                key: Some(key.to_string()),
                message: format!("key {key:?} is not a recognized anyllm-proxy variable"),
            });
        }

        // Warn: sensitive key overwrite.
        if SENSITIVE_KEYS.contains(&key) {
            warnings.push(EnvWarning {
                line: Some(lineno),
                key: Some(key.to_string()),
                message: format!("key {key:?} is sensitive — verify the value is intentional"),
            });
        }

        // Warn: duplicate key (last wins).
        if seen_keys.contains(key) {
            warnings.push(EnvWarning {
                line: Some(lineno),
                key: Some(key.to_string()),
                message: format!("key {key:?} appears more than once; last value wins"),
            });
        } else {
            seen_keys.insert(key.to_string());
        }

        pairs.push(ParsedPair {
            key: key.to_string(),
            value,
            line: lineno,
        });
    }

    // Hard reject: non-empty file that yielded zero valid pairs (likely wrong file type).
    if had_content && pairs.is_empty() && hard_errors.is_empty() {
        hard_errors.push(
            "no valid KEY=VALUE entries found — is this a .anyllm.env file?".to_string(),
        );
    }

    ParseResult {
        pairs,
        warnings,
        hard_errors,
    }
}

/// Check that a key is a valid POSIX shell variable name: `[A-Z_][A-Z0-9_]*`.
/// Lowercase letters are intentionally excluded — all proxy vars are uppercase.
fn is_valid_key(key: &str) -> bool {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if c.is_ascii_uppercase() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_pairs() {
        let result = parse_env_content("BACKEND=openai\nRUST_LOG=info\n");
        assert!(result.hard_errors.is_empty());
        assert_eq!(result.pairs.len(), 2);
        assert_eq!(result.pairs[0].key, "BACKEND");
        assert_eq!(result.pairs[0].value, "openai");
    }

    #[test]
    fn parse_double_quoted_escapes() {
        let result = parse_env_content(r#"MY_KEY="hello\nworld""#);
        assert!(result.hard_errors.is_empty());
        assert_eq!(result.pairs[0].value, "hello\nworld");
    }

    #[test]
    fn parse_single_quoted_literal() {
        let result = parse_env_content("MY_KEY='hello\\nworld'");
        assert!(result.hard_errors.is_empty());
        // Single-quoted: no escape processing
        assert_eq!(result.pairs[0].value, "hello\\nworld");
    }

    #[test]
    fn parse_export_prefix() {
        let result = parse_env_content("export BACKEND=openai");
        assert!(result.hard_errors.is_empty());
        assert_eq!(result.pairs[0].key, "BACKEND");
    }

    #[test]
    fn parse_comments_and_blanks_ignored() {
        let result = parse_env_content("# comment\n\nBACKEND=openai\n");
        assert_eq!(result.pairs.len(), 1);
    }

    #[test]
    fn hard_reject_null_bytes() {
        let content = "BACKEND=openai\x00";
        let result = parse_env_content(content);
        assert!(!result.hard_errors.is_empty());
        assert!(result.hard_errors[0].contains("binary content"));
    }

    #[test]
    fn hard_reject_invalid_key_with_dash() {
        let result = parse_env_content("KEY-WITH-DASH=value");
        assert!(!result.hard_errors.is_empty());
        assert!(result.hard_errors[0].contains("KEY-WITH-DASH"));
    }

    #[test]
    fn hard_reject_lowercase_key() {
        let result = parse_env_content("mykey=value");
        assert!(!result.hard_errors.is_empty());
    }

    #[test]
    fn hard_reject_empty_file_with_content() {
        // All comment lines — had_content stays false, so no "zero pairs" error
        let result = parse_env_content("# just a comment\n");
        assert!(result.hard_errors.is_empty());
    }

    #[test]
    fn hard_reject_no_valid_pairs_from_content() {
        // A line with content but no = is a malformed line (warning), not hard error.
        // But if that's the ONLY content, we get zero pairs + had_content=true.
        let result = parse_env_content("not_an_env_var_line\n");
        // "not_an_env_var_line" has no `=`, so it's a malformed-line warning.
        // had_content=true, pairs empty, hard_errors empty → zero-pairs hard error added.
        assert!(!result.hard_errors.is_empty());
        assert!(result.hard_errors[0].contains("no valid KEY=VALUE"));
    }

    #[test]
    fn warn_unknown_key() {
        let result = parse_env_content("UNKNOWN_XYZ_KEY=value");
        assert!(result.hard_errors.is_empty());
        assert!(result.warnings.iter().any(|w| w.message.contains("not a recognized")));
    }

    #[test]
    fn warn_duplicate_key() {
        let result = parse_env_content("BACKEND=openai\nBACKEND=vertex\n");
        assert!(result.warnings.iter().any(|w| w.message.contains("more than once")));
        // Last value wins
        assert_eq!(result.pairs.last().unwrap().value, "vertex");
    }

    #[test]
    fn warn_empty_value() {
        let result = parse_env_content("BACKEND=");
        assert!(result.warnings.iter().any(|w| w.message.contains("empty value")));
    }

    #[test]
    fn warn_sensitive_key() {
        let result = parse_env_content("ADMIN_TOKEN=secret");
        assert!(result.warnings.iter().any(|w| w.message.contains("sensitive")));
    }

    #[test]
    fn escape_for_env_file_basic() {
        assert_eq!(escape_for_env_file(r#"say "hi""#), r#"say \"hi\""#);
        assert_eq!(escape_for_env_file("line1\nline2"), "line1\\nline2");
        assert_eq!(escape_for_env_file("back\\slash"), "back\\\\slash");
    }

    #[test]
    fn is_valid_key_checks() {
        assert!(is_valid_key("BACKEND"));
        assert!(is_valid_key("_PRIVATE"));
        assert!(is_valid_key("KEY_123"));
        assert!(!is_valid_key("key"));
        assert!(!is_valid_key("KEY-DASH"));
        assert!(!is_valid_key("123KEY"));
        assert!(!is_valid_key(""));
    }

    #[test]
    fn value_with_equals_sign() {
        // First `=` is the delimiter; rest of line is the value
        let result = parse_env_content("BACKEND=open=ai");
        assert!(result.hard_errors.is_empty());
        assert_eq!(result.pairs[0].value, "open=ai");
    }
}
