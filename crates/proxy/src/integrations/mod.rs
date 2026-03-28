// Named integration registry.
//
// Each named integration is initialized at startup and called
// in the same fire-and-forget path as webhook URL callbacks.

pub mod langfuse;

pub use langfuse::LangfuseClient;

/// A named (non-URL) callback integration.
pub enum NamedIntegration {
    Langfuse(std::sync::Arc<LangfuseClient>),
}

impl NamedIntegration {
    /// Send a request log entry to the integration. Fire-and-forget.
    pub fn notify(&self, entry: &crate::admin::state::RequestLogEntry) {
        match self {
            NamedIntegration::Langfuse(client) => client.send(entry),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn langfuse_integration_constructs() {
        // LangfuseClient::from_env returns None when env vars are absent.
        unsafe {
            std::env::remove_var("LANGFUSE_PUBLIC_KEY");
            std::env::remove_var("LANGFUSE_SECRET_KEY");
        }
        assert!(LangfuseClient::from_env().is_none());
    }
}
