use crate::tools::registry::Tool;
use serde_json::Value;
use std::process::Stdio;
use tokio::process::Command;

/// Hard cap on subprocess wall-clock time (seconds).
const TIMEOUT_SECS: u64 = 30;

/// Maximum combined stdout+stderr size returned (bytes). Output beyond this is truncated.
const MAX_OUTPUT_BYTES: usize = 256 * 1024;

/// Tool for executing bash commands.
pub struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &str {
        "execute_bash"
    }

    fn description(&self) -> &str {
        "Executes a bash command and returns the stdout and stderr. Use this to run scripts, search files, etc."
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to execute in bash"
                }
            },
            "required": ["command"]
        })
    }

    fn execute<'a>(
        &'a self,
        input: Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + Send + 'a>>
    {
        Box::pin(async move {
            let command = input
                .get("command")
                .and_then(|v| v.as_str())
                .ok_or_else(|| "Missing 'command' argument".to_string())?;

            let fut = Command::new("bash")
                .arg("-c")
                .arg(command)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output();

            let output = match tokio::time::timeout(
                std::time::Duration::from_secs(TIMEOUT_SECS),
                fut,
            )
            .await
            {
                Ok(Ok(output)) => output,
                Ok(Err(e)) => return Err(format!("Failed to spawn process: {}", e)),
                Err(_) => {
                    return Err(format!(
                        "Command timed out after {} seconds",
                        TIMEOUT_SECS
                    ))
                }
            };

            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            let mut result = String::new();
            if !stdout.is_empty() {
                result.push_str("Stdout:\n");
                result.push_str(&stdout);
            }
            if !stderr.is_empty() {
                if !result.is_empty() {
                    result.push('\n');
                }
                result.push_str("Stderr:\n");
                result.push_str(&stderr);
            }

            if result.is_empty() {
                result = format!(
                    "Command executed successfully with exit code {}",
                    output.status.code().unwrap_or(0)
                );
            }

            // Truncate to avoid blowing up context with huge outputs
            if result.len() > MAX_OUTPUT_BYTES {
                result.truncate(MAX_OUTPUT_BYTES);
                result.push_str("\n... [output truncated]");
            }

            Ok(serde_json::json!({ "output": result }))
        })
    }
}
