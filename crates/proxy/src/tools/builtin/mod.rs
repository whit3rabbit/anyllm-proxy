pub mod bash;
pub mod read_file;

use crate::tools::registry::ToolRegistry;

/// Convenience function to populate a registry with standard tools.
pub fn register_all(registry: &mut ToolRegistry) {
    registry.register(Box::new(bash::BashTool));
    registry.register(Box::new(read_file::ReadFileTool));
}
