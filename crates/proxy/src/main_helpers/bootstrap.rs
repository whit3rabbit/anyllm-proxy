use std::path::{Path, PathBuf};

pub use crate::config::helpers::resolve_data_dir;

/// Resolve SQLite DB path: ADMIN_DB_PATH env var > data_dir/admin.db.
pub fn resolve_db_path(data_dir: &Path) -> String {
    std::env::var("ADMIN_DB_PATH")
        .unwrap_or_else(|_| data_dir.join("admin.db").to_string_lossy().into_owned())
}

/// Resolve admin token file path from `ADMIN_TOKEN_PATH` env var,
/// falling back to `~/.anyllm/.admin_token`.
pub fn resolve_admin_token_path(data_dir: &Path) -> PathBuf {
    match std::env::var("ADMIN_TOKEN_PATH") {
        Ok(p) => {
            let path = PathBuf::from(&p);
            // Reject paths containing traversal sequences to prevent writing
            // the admin token to unexpected locations via misconfigured env vars.
            if p.contains("..") {
                panic!("ADMIN_TOKEN_PATH must not contain '..' path traversal: {p}");
            }
            path
        }
        Err(_) => data_dir.join(".admin_token"),
    }
}

/// Write the admin token to a file with mode 0600 (owner-only read/write).
/// On Unix, sets permissions atomically at creation to avoid a TOCTOU race
/// where the file is briefly world-readable before chmod.
pub fn write_token_file(path: &str, token: &str) -> std::io::Result<()> {
    use std::io::Write;

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
    };

    #[cfg(not(unix))]
    let mut file: std::fs::File = {
        // On non-Unix platforms, file permissions cannot be set to owner-only
        // at creation time. Returning an error forces the caller to panic,
        // requiring the operator to set ADMIN_TOKEN explicitly.
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "auto-generating the admin token file is not supported on non-Unix platforms; \
             set the ADMIN_TOKEN environment variable explicitly",
        ));
    };

    file.write_all(token.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

/// Parse a `.env`-format file and return `(key, value)` pairs to set.
///
/// Delegates parsing to `anyllm_proxy::env_parser::parse_env_content` (pure, no side effects).
///
/// Hard errors are printed to stderr and result in an empty list; warnings are printed as-is.
/// Already-set environment variables are skipped so the real environment always wins.
/// Compatible with Docker `--env-file` and standard dotenv tooling.
pub fn parse_env_file(path: &str) -> Vec<(String, String)> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("anyllm_proxy: could not read env file '{path}': {e}");
            return Vec::new();
        }
    };

    let result = anyllm_proxy::env_parser::parse_env_content(&content);

    for err in &result.hard_errors {
        eprintln!("anyllm_proxy: {path}: {err}");
    }
    if !result.hard_errors.is_empty() {
        return Vec::new();
    }

    for warn in &result.warnings {
        let loc = warn.line.map(|l| format!(":{l}")).unwrap_or_default();
        eprintln!("anyllm_proxy: {path}{loc}: {}", warn.message);
    }

    result
        .pairs
        .into_iter()
        .filter(|p| std::env::var(&p.key).is_err())
        .map(|p| (p.key, p.value))
        .collect()
}

/// Load env vars previously imported via the admin UI from the SQLite `env_import` table.
///
/// Opens the database synchronously (rusqlite is sync) before the tokio runtime starts,
/// so `set_var` remains single-threaded safe. Skips keys already present in the environment
/// (real env and .anyllm.env take precedence). Silently succeeds if the DB or table does
/// not yet exist (first run before any import).
pub fn load_env_from_sqlite(db_path: &str) -> Vec<(String, String)> {
    let conn = match rusqlite::Connection::open(db_path) {
        Ok(c) => c,
        Err(e) => {
            // Benign when the DB file doesn't exist yet (first run).
            // Log for any other open failure (permissions, corruption, etc.).
            if std::path::Path::new(db_path).exists() {
                eprintln!("anyllm_proxy: could not open DB '{db_path}' for env import: {e}");
            }
            return Vec::new();
        }
    };

    // The env_import table is created by init_db() during normal startup.
    // If the proxy has never run with a DB, the table won't exist yet.
    let rows: Vec<(String, String)> =
        match conn.prepare("SELECT key, value FROM env_import ORDER BY key") {
            Ok(mut stmt) => stmt
                .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
                .and_then(|mapped| mapped.collect())
                .unwrap_or_default(),
            Err(_) => return Vec::new(), // Table doesn't exist yet
        };

    rows.into_iter()
        .filter(|(key, _)| std::env::var(key).is_err())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_file_double_quoted_newline_escape() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_parse_env_escape_n.env");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"KEY="hello\nworld""#).unwrap();
        drop(f);
        let vars = parse_env_file(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        let val = vars
            .iter()
            .find(|(k, _)| k == "KEY")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            val,
            Some("hello\nworld"),
            "\\n inside double quotes must become a newline"
        );
    }

    #[test]
    fn parse_env_file_double_quoted_tab_escape() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_parse_env_escape_t.env");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"KEY="col1\tcol2""#).unwrap();
        drop(f);
        let vars = parse_env_file(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        let val = vars
            .iter()
            .find(|(k, _)| k == "KEY")
            .map(|(_, v)| v.as_str());
        assert_eq!(
            val,
            Some("col1\tcol2"),
            "\\t inside double quotes must become a tab"
        );
    }

    #[test]
    fn parse_env_file_single_quoted_no_escape() {
        use std::io::Write;
        let dir = std::env::temp_dir();
        let path = dir.join("test_parse_env_single.env");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r#"KEY='hello\nworld'"#).unwrap();
        drop(f);
        let vars = parse_env_file(path.to_str().unwrap());
        std::fs::remove_file(&path).ok();
        let val = vars
            .iter()
            .find(|(k, _)| k == "KEY")
            .map(|(_, v)| v.as_str());
        // Single quotes: backslash is literal, no escape processing.
        assert_eq!(
            val,
            Some(r"hello\nworld"),
            "single quotes must not process escapes"
        );
    }
}
