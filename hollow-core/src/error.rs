// hollow-core/src/error.rs
use thiserror::Error;

#[derive(Debug, Error, uniffi::Error)]
pub enum HollowError {
    #[error("database error: {0}")]
    Database(String),
    #[error("file not found: {0}")]
    FileNotFound(String),
    #[error("duplicate file: {0}")]
    DuplicateFile(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

impl From<rusqlite::Error> for HollowError {
    fn from(e: rusqlite::Error) -> Self {
        HollowError::Database(e.to_string())
    }
}
