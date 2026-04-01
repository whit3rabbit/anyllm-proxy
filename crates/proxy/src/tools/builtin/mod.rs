// SAFETY: The tools in this module execute arbitrary shell commands (BashTool)
// and read arbitrary files (ReadFileTool) as the proxy process user. They must
// NEVER be registered in a server-side tool registry without explicit operator
// opt-in and appropriate sandboxing. Gated behind the `dangerous-builtin-tools`
// feature flag, which is OFF by default.

#[cfg(feature = "dangerous-builtin-tools")]
pub mod bash;
#[cfg(feature = "dangerous-builtin-tools")]
pub mod read_file;

use crate::tools::registry::ToolRegistry;

/// Populate a registry with standard built-in tools.
///
/// `builtin_configs`: optional map of tool name -> config from PROXY_CONFIG; used to
/// pass per-tool settings (e.g., `allowed_dirs` for `read_file`) to tool constructors.
///
/// # Safety
///
/// When `dangerous-builtin-tools` is enabled, this registers `BashTool` (arbitrary
/// shell execution) and `ReadFileTool` (arbitrary file reads). Only call this if
/// the tool execution engine is sandboxed or if the operator has explicitly opted in.
///
/// When the feature is disabled (the default), this is a no-op.
pub fn register_all(
    _registry: &mut ToolRegistry,
    _builtin_configs: Option<
        &std::collections::HashMap<String, crate::config::simple::BuiltinToolConfig>,
    >,
) {
    #[cfg(feature = "dangerous-builtin-tools")]
    {
        _registry.register(Box::new(bash::BashTool));

        // Build ReadFileTool with allowed_dirs from config, canonicalized at registration time.
        let allowed_dirs = _builtin_configs
            .and_then(|m| m.get("read_file"))
            .map(|cfg| {
                cfg.allowed_dirs
                    .iter()
                    .filter_map(|d| {
                        let p = std::path::PathBuf::from(d);
                        match p.canonicalize() {
                            Ok(canon) => Some(canon),
                            Err(e) => {
                                tracing::warn!(
                                    dir = %d,
                                    error = %e,
                                    "read_file allowed_dirs entry could not be canonicalized; skipping"
                                );
                                None
                            }
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        _registry.register(Box::new(read_file::ReadFileTool { allowed_dirs }));
    }
}
