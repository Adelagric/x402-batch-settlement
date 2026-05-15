//! x402 V2 wire types.
//!
//! `PaymentRequired` is the Base64-encoded body of the `PAYMENT-REQUIRED`
//! header on a 402 response. `PaymentPayload` is the Base64-encoded body
//! of the `PAYMENT-SIGNATURE` header sent by the client on retry. The
//! scheme-specific `payload` is opaque at this layer; see `exact_evm`.

use serde::{Deserialize, Serialize};

use crate::error::Error;

/// Protocol version this crate implements.
pub const X402_VERSION: u8 = 2;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Resource {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
}

/// Scheme-specific hints carried alongside the requirements. For
/// `exact`/EVM this is token metadata (`name`, `version`) and, on the
/// client payload, optionally the chosen `assetTransferMethod`. The
/// canonical `PaymentRequired` does not carry `assetTransferMethod`, so
/// it is optional here (verified against specs/transports-v2/http.md).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Extra {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_transfer_method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// batch-settlement: address authorizing claims/refunds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_authorizer: Option<String>,
    /// batch-settlement: channel withdraw delay in seconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdraw_delay: Option<u64>,
}

/// A single accepted payment option. Appears both in the `accepts`
/// array of `PaymentRequired` and as the `accepted` option echoed back
/// in `PaymentPayload`. `amount` is the atomic on-chain amount as a
/// decimal string (not a display price).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: String,
    pub network: String,
    pub amount: String,
    pub asset: String,
    pub pay_to: String,
    pub max_timeout_seconds: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<Extra>,
}

/// Body of the `PAYMENT-REQUIRED` header on a 402 response. `error`
/// carries a human-readable reason on the initial challenge (e.g.
/// "PAYMENT-SIGNATURE header is required").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequired {
    pub x402_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub accepts: Vec<PaymentRequirements>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<Resource>,
}

/// Body of the `PAYMENT-SIGNATURE` header sent by the client on retry.
/// `payload` is scheme-specific and decoded by the relevant scheme
/// module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub x402_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<Resource>,
    pub accepted: PaymentRequirements,
    pub payload: serde_json::Value,
}

impl PaymentPayload {
    /// Reject payloads that do not target the protocol version this
    /// crate implements.
    pub fn check_version(&self) -> Result<(), Error> {
        if self.x402_version != X402_VERSION {
            return Err(Error::UnsupportedVersion {
                expected: X402_VERSION,
                got: self.x402_version,
            });
        }
        Ok(())
    }
}

/// Body of the `PAYMENT-RESPONSE` header, and the response of the
/// facilitator `/settle` endpoint. `transaction` is empty on failure;
/// `error_reason` is present only on failure. Verified against
/// specs/transports-v2/http.md and go/FACILITATOR.md.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlementResponse {
    pub success: bool,
    #[serde(default)]
    pub transaction: String,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub payer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    /// Facilitator-provided failure detail (e.g. contract revert text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    /// On-chain amount moved (deposit/settle/refund); empty/absent for
    /// voucher-only responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<String>,
    /// Scheme-specific. For `batch-settlement`: `chargedAmount` and the
    /// `channelState` snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}
