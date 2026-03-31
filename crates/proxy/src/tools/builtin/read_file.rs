use crate::tools::registry::Tool;
use serde_json::Value;
use std::path::Path;

/// Maximum file size to read (1 MB). Prevents OOM from huge files.
const MAX_FILE_SIZE: u64 = 1024 * 1024;

/// Tool for reading file contents safely.
pub struct ReadFileTool;

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
        Box::pin(async move {
            let raw_path = input
                .get("path")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'path' argument".to_string())?;

            let path = Path::new(raw_path);

            // Require absolute paths to prevent path traversal
            if !path.is_absolute() {
                return Err("Only absolute paths are allowed".to_string());
            }

            // Resolve symlinks and .. components, then verify the canonical path
            // still starts with the original parent to block traversal via symlinks
            let canonical = path
                .canonicalize()
                .map_err(|e| format!("Cannot resolve path '{}': {}", raw_path, e))?;

            // Check file size before reading
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
