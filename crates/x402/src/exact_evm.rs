//! `exact` scheme on EVM, EIP-3009 asset transfer method.
//!
//! The client signs an EIP-3009 `transferWithAuthorization`; the
//! facilitator broadcasts it and pays gas. The facilitator cannot alter
//! amount or destination. Spec: specs/schemes/exact/scheme_exact_evm.md.

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::payment::PaymentPayload;

pub const SCHEME_EXACT: &str = "exact";
pub const ASSET_TRANSFER_EIP3009: &str = "eip3009";
pub const ASSET_TRANSFER_PERMIT2: &str = "permit2";
pub const ASSET_TRANSFER_ERC7710: &str = "erc7710";

/// Parameters required to reconstruct the signed EIP-3009 message.
/// All numeric values are decimal strings to preserve 256-bit range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Eip3009Authorization {
    pub from: String,
    pub to: String,
    pub value: String,
    pub valid_after: String,
    pub valid_before: String,
    pub nonce: String,
}

/// The `payload` of a `PaymentPayload` for `exact`/EVM with the
/// `eip3009` asset transfer method.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExactEvmEip3009Payload {
    pub signature: String,
    pub authorization: Eip3009Authorization,
}

impl ExactEvmEip3009Payload {
    /// Extract and decode the EIP-3009 payload from a payment, after
    /// checking the protocol version.
    pub fn from_payment(payment: &PaymentPayload) -> Result<Self, Error> {
        payment.check_version()?;
        serde_json::from_value(payment.payload.clone())
            .map_err(|e| Error::InvalidPayload(e.to_string()))
    }
}
