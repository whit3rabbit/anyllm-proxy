//! Verbatim fact-sheet. Port of pxpipe's `factsheet.ts` (char-heuristic variant,
//! no regex dep).
//!
//! When the slab is imaged, the precision-critical, hard-to-OCR strings inside
//! it — file paths, URLs, SHAs/UUIDs, versions, CLI flags, CONST_IDS, big
//! numbers — are exactly what a model most needs to quote verbatim yet most
//! likely to misread off tiny glyphs. This extracts those tokens so they ride
//! next to the image as plain text. Deterministic (fixed order, lexical sort,
//! no time/rng) so the emitted line is byte-stable and never busts the cache.

/// Max identifiers kept (highest-priority first). Mirrors pxpipe's budget.
const MAX_TOKENS: usize = 64;
/// Whitespace-free chunks longer than this are blobs (base64, minified) — skip.
const MAX_CHUNK: usize = 512;

/// Priority tier by SHAPE (lower = kept first): short opaque identifiers
/// (SHAs/ports/flags) outrank long, reconstructable URLs when the budget binds.
fn tier(tok: &str) -> u8 {
    if is_url(tok) {
        2
    } else if is_hex_sha(tok) || is_uuid(tok) || is_const_id(tok) || is_flag(tok) || is_number(tok)
    {
        0
    } else {
        1
    }
}

fn is_url(t: &str) -> bool {
    t.starts_with("http://") || t.starts_with("https://")
}
fn is_path(t: &str) -> bool {
    t.contains('/') && t.len() > 2 && !is_url(t)
}
fn is_hex_sha(t: &str) -> bool {
    // Bare hex hash: SHA-1 (40) through SHA-512 (128). Requires a digit so plain
    // hex-letter English words ("deadbeef" aside) don't flood the sheet.
    (7..=128).contains(&t.len())
        && t.chars().all(|c| c.is_ascii_hexdigit())
        && t.chars().any(|c| c.is_ascii_digit())
}
/// Canonical 8-4-4-4-12 hex UUID (dashes make it fail is_hex_sha).
fn is_uuid(t: &str) -> bool {
    let groups: Vec<&str> = t.split('-').collect();
    groups.len() == 5
        && [8usize, 4, 4, 4, 12]
            .iter()
            .zip(&groups)
            .all(|(&n, g)| g.len() == n && g.chars().all(|c| c.is_ascii_hexdigit()))
}
fn is_const_id(t: &str) -> bool {
    t.len() >= 4
        && t.contains('_')
        && t.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
        && t.chars().any(|c| c.is_ascii_uppercase())
}
fn is_flag(t: &str) -> bool {
    (t.starts_with("--") || t.starts_with('-'))
        && t.len() >= 3
        && t[1..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '-')
}
fn is_number(t: &str) -> bool {
    let core = t.trim_matches(|c: char| c == ',' || c == '.');
    core.len() >= 4
        && core
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
        && core.chars().any(|c| c.is_ascii_digit())
}
fn is_version(t: &str) -> bool {
    let t = t.strip_prefix('v').unwrap_or(t);
    let dots = t.matches('.').count();
    dots >= 1
        && t.chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '+')
        && t.chars().next().is_some_and(|c| c.is_ascii_digit())
}

/// Notable token = anything a model would need to quote exactly.
fn is_notable(t: &str) -> bool {
    is_url(t)
        || is_path(t)
        || is_hex_sha(t)
        || is_uuid(t)
        || is_const_id(t)
        || is_flag(t)
        || is_number(t)
        || is_version(t)
}

/// Trim trailing sentence punctuation pulled in from prose.
fn trim_token(t: &str) -> &str {
    t.trim_matches(|c: char| matches!(c, '.' | ',' | ';' | ':' | ')' | '(' | '"' | '\'' | '`'))
}

/// Extract deduped precision-critical tokens from `text`, deterministically
/// ordered (tier, then lexical) and capped at `MAX_TOKENS`.
pub fn extract_tokens(text: &str) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    for chunk in text.split_whitespace() {
        if chunk.len() > MAX_CHUNK {
            continue;
        }
        let tok = trim_token(chunk);
        if tok.len() >= 3 && is_notable(tok) {
            seen.insert(tok.to_string());
        }
    }
    let mut toks: Vec<String> = seen.into_iter().collect();
    // Stable total order: tier asc, then lexical (BTreeSet already gave lexical).
    toks.sort_by(|a, b| tier(a).cmp(&tier(b)).then_with(|| a.cmp(b)));
    toks.truncate(MAX_TOKENS);
    toks
}

/// One-line fact-sheet string, or empty when nothing notable was found.
pub fn fact_sheet_text(text: &str) -> String {
    let toks = extract_tokens(text);
    if toks.is_empty() {
        return String::new();
    }
    format!("Verbatim identifiers (quote exactly): {}", toks.join(" "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_paths_shas_flags() {
        let t = "See src/lib.rs and commit a1b2c3d4 with --no-verify flag; PROXY_CONFIG set.";
        let toks = extract_tokens(t);
        assert!(toks.iter().any(|x| x == "src/lib.rs"));
        assert!(toks.iter().any(|x| x == "a1b2c3d4"));
        assert!(toks.iter().any(|x| x == "--no-verify"));
        assert!(toks.iter().any(|x| x == "PROXY_CONFIG"));
    }

    #[test]
    fn deterministic_and_capped() {
        let t = "src/a.rs src/b.rs src/a.rs v1.2.3";
        let a = extract_tokens(t);
        let b = extract_tokens(t);
        assert_eq!(a, b);
        assert!(a.contains(&"src/a.rs".to_string()));
        assert_eq!(a.iter().filter(|x| *x == "src/a.rs").count(), 1); // deduped
    }

    #[test]
    fn extracts_uuid_and_sha256() {
        let t = "run 550e8400-e29b-41d4-a716-446655440000 digest \
                 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08 done";
        let toks = extract_tokens(t);
        assert!(
            toks.iter()
                .any(|x| x == "550e8400-e29b-41d4-a716-446655440000"),
            "uuid missed"
        );
        assert!(toks.iter().any(|x| x.len() == 64), "sha-256 missed");
    }

    #[test]
    fn empty_when_prose_only() {
        assert_eq!(fact_sheet_text("just some plain english words here"), "");
    }
}
