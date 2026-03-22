use thiserror::Error;

/// Errors that can occur during API format translation.
#[derive(Error, Debug, Clone)]
pub enum TranslateError {
    /// The model name did not match any entry in the translation config.
    #[error("unknown model: {0}")]
    UnknownModel(String),

    /// A translation step failed (validation, unsupported feature with strict config, etc.).
    #[error("translation error: {0}")]
    Translation(String),
}
