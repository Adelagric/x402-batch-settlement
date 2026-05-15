//! Header codec. All three x402 V2 payment headers carry Base64-encoded
//! JSON. These helpers are the single place that encoding is applied.

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{de::DeserializeOwned, Serialize};

use crate::error::Error;

pub const HEADER_PAYMENT_REQUIRED: &str = "PAYMENT-REQUIRED";
pub const HEADER_PAYMENT_SIGNATURE: &str = "PAYMENT-SIGNATURE";
pub const HEADER_PAYMENT_RESPONSE: &str = "PAYMENT-RESPONSE";

/// Serialize to JSON then Base64, for use as a header value.
pub fn encode_header<T: Serialize>(value: &T) -> Result<String, Error> {
    let json = serde_json::to_vec(value).map_err(|e| Error::Json(e.to_string()))?;
    Ok(STANDARD.encode(json))
}

/// Decode a Base64 header value and deserialize the JSON it wraps.
pub fn decode_header<T: DeserializeOwned>(raw: &str) -> Result<T, Error> {
    let bytes = STANDARD
        .decode(raw.trim())
        .map_err(|e| Error::Base64(e.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|e| Error::Json(e.to_string()))
}
