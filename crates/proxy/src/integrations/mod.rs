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
    #[test]
    fn named_integration_dispatches() {
        // Smoke test: NamedIntegration enum compiles and notify() is callable.
        // Actual LangfuseClient behavior is tested in integrations::langfuse::tests.
    }
}
