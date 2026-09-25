use serde::Serialize;

/// Pipeline stage at which an error occurred, per design doc §3/§6.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Input,
    Extraction,
    Classification,
    Structuring,
    Validation,
    Output,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("invalid input: {message}")]
    InvalidInput { code: &'static str, message: String },

    #[error("unsupported format: {message}")]
    UnsupportedFormat { code: &'static str, message: String },

    #[error("extraction failed: {message}")]
    Extraction { code: &'static str, message: String },

    #[error("classification failed: {message}")]
    Classification { code: &'static str, message: String },

    #[error("structuring failed: {message}")]
    Structuring { code: &'static str, message: String },

    #[error("validation failed: {message}")]
    Validation { code: &'static str, message: String },
}

impl EngineError {
    pub fn stage(&self) -> Stage {
        match self {
            EngineError::InvalidInput { .. } => Stage::Input,
            EngineError::UnsupportedFormat { .. } => Stage::Input,
            EngineError::Extraction { .. } => Stage::Extraction,
            EngineError::Classification { .. } => Stage::Classification,
            EngineError::Structuring { .. } => Stage::Structuring,
            EngineError::Validation { .. } => Stage::Validation,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            EngineError::InvalidInput { code, .. }
            | EngineError::UnsupportedFormat { code, .. }
            | EngineError::Extraction { code, .. }
            | EngineError::Classification { code, .. }
            | EngineError::Structuring { code, .. }
            | EngineError::Validation { code, .. } => code,
        }
    }

    pub fn message(&self) -> String {
        match self {
            EngineError::InvalidInput { message, .. }
            | EngineError::UnsupportedFormat { message, .. }
            | EngineError::Extraction { message, .. }
            | EngineError::Classification { message, .. }
            | EngineError::Structuring { message, .. }
            | EngineError::Validation { message, .. } => message.clone(),
        }
    }

    /// Coarse-grained process exit code, per design doc §6.
    pub fn exit_code(&self) -> i32 {
        match self {
            EngineError::InvalidInput { .. } => 2,
            EngineError::UnsupportedFormat { .. } => 3,
            EngineError::Extraction { .. } => 4,
            EngineError::Classification { .. } => 5,
            EngineError::Structuring { .. } => 5,
            EngineError::Validation { .. } => 5,
        }
    }
}
