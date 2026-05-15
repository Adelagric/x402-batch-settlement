//! Transport-agnostic facilitator contract.
//!
//! The resource server POSTs the payment payload and the matched
//! requirements to the facilitator's `/verify` then `/settle`. This
//! module defines the request/response types and a transport trait; it
//! deliberately does not depend on any HTTP client. A concrete
//! transport (reqwest, etc.) is provided by the consumer.
//!
//! Schemas verified against specs/transports-v2/http.md and
//! go/FACILITATOR.md.

use serde::{Deserialize, Serialize};

use crate::error::Error;
use crate::payment::{PaymentPayload, PaymentRequirements, SettlementResponse};

/// Request body for both `/verify` and `/settle`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FacilitatorRequest {
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

impl FacilitatorRequest {
    pub fn new(payment_payload: PaymentPayload, payment_requirements: PaymentRequirements) -> Self {
        Self {
            payment_payload,
            payment_requirements,
        }
    }
}

/// Response of `/verify`. `invalid_reason` is an empty string when the
/// payment is valid.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub is_valid: bool,
    #[serde(default)]
    pub invalid_reason: String,
    /// Scheme-specific snapshot. For `batch-settlement`, the facilitator
    /// returns the channel state (balance, totalClaimed, ...) here.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

/// Response of `/settle` is the same shape as the `PAYMENT-RESPONSE`
/// body.
pub type SettleResponse = SettlementResponse;

/// Implemented by a concrete HTTP transport in the consuming crate.
#[allow(async_fn_in_trait)]
pub trait FacilitatorTransport {
    async fn verify(&self, req: &FacilitatorRequest) -> Result<VerifyResponse, Error>;
    async fn settle(&self, req: &FacilitatorRequest) -> Result<SettleResponse, Error>;
}
