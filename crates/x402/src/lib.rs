//! `x402` — programmatic payment protocol for HTTP (status code 402).
//!
//! Wire types for x402 **V2** and a transport-agnostic facilitator
//! contract. The core is intentionally minimal: it does not depend on
//! any HTTP client. Concrete facilitator transports are provided by
//! consumers.
//!
//! Spec: <https://github.com/x402-foundation/x402> (specs/schemes/exact).

#![forbid(unsafe_code)]

pub mod batch_settlement;
pub mod codec;
pub mod error;
pub mod exact_evm;
pub mod facilitator;
pub mod network;
pub mod payment;

pub use error::Error;
pub use payment::{
    PaymentPayload, PaymentRequired, PaymentRequirements, SettlementResponse, X402_VERSION,
};
