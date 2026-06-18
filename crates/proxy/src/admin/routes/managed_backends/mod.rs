mod config;
mod handlers;

pub use crate::admin::db::{ManagedBackendPatch, ManagedBackendRow};

pub use config::{row_to_backend_config, ManagedBackendConfigError};
pub use handlers::{
    create, delete, list, update, CreateManagedBackendRequest, ManagedBackendResponse,
};
