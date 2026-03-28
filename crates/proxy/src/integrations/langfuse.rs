// Langfuse integration stub (implementation in Task 3).

use std::sync::Arc;

pub struct LangfuseClient;

impl LangfuseClient {
    pub fn from_env() -> Option<Arc<Self>> {
        None
    }

    pub fn send(&self, _entry: &crate::admin::state::RequestLogEntry) {}
}
