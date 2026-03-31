use crate::tools::registry::Tool;
use serde_json::Value;
use std::process::Stdio;
use tokio::process::Command;

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

            // Using standard tokio Command for async execution
            // Timeouts should ideally be added in production
            let output = match Command::new("bash")
                .arg("-c")
                .arg(command)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .output()
                .await
            {
                Ok(output) => output,
                Err(e) => return Err(format!("Failed to spawn process: {}", e)),
            };

            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();

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

            Ok(serde_json::json!({ "output": result }))
        })
    }
}
