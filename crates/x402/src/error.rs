use thiserror::Error;

/// Errors surfaced by the x402 core.
#[derive(Debug, Error)]
pub enum Error {
    #[error("invalid payment payload: {0}")]
    InvalidPayload(String),

    #[error("unsupported x402 version: expected {expected}, got {got}")]
    UnsupportedVersion { expected: u8, got: u8 },

    #[error("base64 decode failed: {0}")]
    Base64(String),

    #[error("json (de)serialization failed: {0}")]
    Json(String),

    #[error("facilitator error: {0}")]
    Facilitator(String),

    #[error("batch-settlement: {0}")]
    BatchSettlement(String),
}
