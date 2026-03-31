use crate::tools::registry::Tool;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Maximum file size to read (1 MB). Prevents OOM from huge files.
const MAX_FILE_SIZE: u64 = 1024 * 1024;

/// Tool for reading file contents safely.
pub struct ReadFileTool {
    /// If non-empty, restrict reads to files under these directories.
    /// All entries must be canonical absolute paths (no symlinks, no ..).
    pub allowed_dirs: Vec<PathBuf>,
}

impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Reads the contents of a local file and returns it as a string."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The absolute path to the file to read."
                }
            },
            "required": ["path"]
        })
    }

    fn execute<'a>(
        &'a self,
        input: Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send + 'a>>
    {
        let allowed_dirs = self.allowed_dirs.clone();
        Box::pin(async move {
            let raw_path = input
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'path' argument".to_string())?;

            let path = Path::new(raw_path);

            // Require absolute paths to prevent relative path traversal.
            if !path.is_absolute() {
                return Err("Only absolute paths are allowed".to_string());
            }

            // Resolve symlinks and .. components.
            let canonical = path
                .canonicalize()
                .map_err(|e| format!("Cannot resolve path '{}': {}", raw_path, e))?;

            // Enforce allowed_dirs allowlist. After canonicalize(), the resolved
            // path must start with at least one of the configured base directories.
            // This blocks both path traversal (../../../etc) and symlink attacks.
            if !allowed_dirs.is_empty() {
                let permitted = allowed_dirs.iter().any(|base| canonical.starts_with(base));
                if !permitted {
                    return Err(format!(
                        "Path '{}' is outside the configured allowed directories",
                        raw_path
                    ));
                }
            } else {
                // No allowed_dirs configured: warn the operator.
                tracing::warn!(
                    path = %raw_path,
                    "read_file executed with no allowed_dirs restriction; \
                     set allowed_dirs in builtin_tools config to restrict file access"
                );
            }

            // Check file size before reading.
            let metadata = std::fs::metadata(&canonical)
                .map_err(|e| format!("Cannot stat '{}': {}", raw_path, e))?;
            if metadata.len() > MAX_FILE_SIZE {
                return Err(format!(
                    "File is {} bytes, exceeds {} byte limit",
                    metadata.len(),
                    MAX_FILE_SIZE
                ));
            }

            match std::fs::read_to_string(&canonical) {
                Ok(content) => Ok(serde_json::json!({ "content": content })),
                Err(e) => Err(format!("Failed to read file '{}': {}", raw_path, e)),
            }
        })
    }
}
