use std::ffi::NulError;
use std::str::Utf8Error;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Failed to load model from {0}")]
    ModelLoadFailed(String),

    #[error("Failed to create context")]
    ContextCreateFailed,

    #[error("Tokenization failed")]
    TokenizeFailed,

    #[error("Decoding failed")]
    DecodeFailed,

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("String conversion failed: {0}")]
    StringError(#[from] NulError),

    #[error("UTF-8 error: {0}")]
    Utf8Error(#[from] Utf8Error),

    #[error("Backend error: {0}")]
    BackendError(String),

    #[error("Context state size invalid")]
    ContextSizeInvalid,
}

pub type Result<T> = std::result::Result<T, Error>;
