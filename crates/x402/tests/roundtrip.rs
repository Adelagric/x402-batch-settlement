//! Golden tests pinned to the canonical spec, plus serde/codec
//! round-trips. The JSON and the verbatim Base64 header values are
//! copied from specs/schemes/exact/scheme_exact_evm.md and
//! specs/transports-v2/http.md.

use proptest::prelude::*;

use x402::codec::{decode_header, encode_header};
use x402::exact_evm::{Eip3009Authorization, ExactEvmEip3009Payload};
use x402::payment::{PaymentPayload, PaymentRequired, SettlementResponse};

const SPEC_EIP3009_PAYLOAD: &str = r#"{
  "x402Version": 2,
  "resource": {
    "url": "https://api.example.com/premium-data",
    "description": "Access to premium market data",
    "mimeType": "application/json"
  },
  "accepted": {
    "scheme": "exact",
    "network": "eip155:84532",
    "amount": "10000",
    "asset": "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
    "payTo": "0x209693Bc6afc0C5328bA36FaF03C514EF312287C",
    "maxTimeoutSeconds": 60,
    "extra": {
      "assetTransferMethod": "eip3009",
      "name": "USDC",
      "version": "2"
    }
  },
  "payload": {
    "signature": "0x2d6a7588d6acca505cbf0d9a4a227e0c52c6c34008c8e8986a1283259764173608a2ce6496642e377d6da8dbbf5836e9bd15092f9ecab05ded3d6293af148b571c",
    "authorization": {
      "from": "0x857b06519E91e3A54538791bDbb0E22373e36b66",
      "to": "0x209693Bc6afc0C5328bA36FaF03C514EF312287C",
      "value": "10000",
      "validAfter": "1740672089",
      "validBefore": "1740672154",
      "nonce": "0xf3746613c2d920b5fdabc0856f2aeb2d4f88ee6037b8cc5d04a71a4462f13480"
    }
  }
}"#;

// Verbatim PAYMENT-SIGNATURE header value from specs/transports-v2/http.md
const SPEC_PAYMENT_SIGNATURE_B64: &str = "eyJ4NDAyVmVyc2lvbiI6MiwicmVzb3VyY2UiOnsidXJsIjoiaHR0cHM6Ly9hcGkuZXhhbXBsZS5jb20vcHJlbWl1bS1kYXRhIiwiZGVzY3JpcHRpb24iOiJBY2Nlc3MgdG8gcHJlbWl1bSBtYXJrZXQgZGF0YSIsIm1pbWVUeXBlIjoiYXBwbGljYXRpb24vanNvbiJ9LCJhY2NlcHRlZCI6eyJzY2hlbWUiOiJleGFjdCIsIm5ldHdvcmsiOiJlaXAxNTU6ODQ1MzIiLCJhbW91bnQiOiIxMDAwMCIsImFzc2V0IjoiMHgwMzZDYkQ1Mzg0MmM1NDI2NjM0ZTc5Mjk1NDFlQzIzMThmM2RDRjdlIiwicGF5VG8iOiIweDIwOTY5M0JjNmFmYzBDNTMyOGJBMzZGYUYwM0M1MTRFRjMxMjI4N0MiLCJtYXhUaW1lb3V0U2Vjb25kcyI6NjAsImV4dHJhIjp7Im5hbWUiOiJVU0RDIiwidmVyc2lvbiI6IjIifX0sInBheWxvYWQiOnsic2lnbmF0dXJlIjoiMHgyZDZhNzU4OGQ2YWNjYTUwNWNiZjBkOWE0YTIyN2UwYzUyYzZjMzQwMDhjOGU4OTg2YTEyODMyNTk3NjQxNzM2MDhhMmNlNjQ5NjY0MmUzNzdkNmRhOGRiYmY1ODM2ZTliZDE1MDkyZjllY2FiMDVkZWQzZDYyOTNhZjE0OGI1NzFjIiwiYXV0aG9yaXphdGlvbiI6eyJmcm9tIjoiMHg4NTdiMDY1MTlFOTFlM0E1NDUzODc5MWJEYmIwRTIyMzczZTM2YjY2IiwidG8iOiIweDIwOTY5M0JjNmFmYzBDNTMyOGJBMzZGYUYwM0M1MTRFRjMxMjI4N0MiLCJ2YWx1ZSI6IjEwMDAwIiwidmFsaWRBZnRlciI6IjE3NDA2NzIwODkiLCJ2YWxpZEJlZm9yZSI6IjE3NDA2NzIxNTQiLCJub25jZSI6IjB4ZjM3NDY2MTNjMmQ5MjBiNWZkYWJjMDg1NmYyYWViMmQ0Zjg4ZWU2MDM3YjhjYzVkMDRhNzFhNDQ2MmYxMzQ4MCJ9fX0=";

// Verbatim PAYMENT-RESPONSE header values from specs/transports-v2/http.md
const SPEC_PAYMENT_RESPONSE_OK_B64: &str = "eyJzdWNjZXNzIjp0cnVlLCJ0cmFuc2FjdGlvbiI6IjB4MTIzNDU2Nzg5MGFiY2RlZjEyMzQ1Njc4OTBhYmNkZWYxMjM0NTY3ODkwYWJjZGVmMTIzNDU2Nzg5MGFiY2RlZiIsIm5ldHdvcmsiOiJlaXAxNTU6ODQ1MzIiLCJwYXllciI6IjB4ODU3YjA2NTE5RTkxZTNBNTQ1Mzg3OTFiRGJiMEUyMjM3M2UzNmI2NiJ9";
const SPEC_PAYMENT_RESPONSE_FAIL_B64: &str = "eyJzdWNjZXNzIjpmYWxzZSwiZXJyb3JSZWFzb24iOiJpbnN1ZmZpY2llbnRfZnVuZHMiLCJ0cmFuc2FjdGlvbiI6IiIsIm5ldHdvcmsiOiJlaXAxNTU6ODQ1MzIiLCJwYXllciI6IjB4ODU3YjA2NTE5RTkxZTNBNTQ1Mzg3OTFiRGJiMEUyMjM3M2UzNmI2NiJ9";

// PaymentRequired JSON from specs/transports-v2/http.md (extra without
// assetTransferMethod, top-level error present).
const SPEC_PAYMENT_REQUIRED: &str = r#"{
  "x402Version": 2,
  "error": "PAYMENT-SIGNATURE header is required",
  "resource": {
    "url": "https://api.example.com/premium-data",
    "description": "Access to premium market data",
    "mimeType": "application/json"
  },
  "accepts": [
    {
      "scheme": "exact",
      "network": "eip155:84532",
      "amount": "10000",
      "asset": "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
      "payTo": "0x209693Bc6afc0C5328bA36FaF03C514EF312287C",
      "maxTimeoutSeconds": 60,
      "extra": { "name": "USDC", "version": "2" }
    }
  ]
}"#;

#[test]
fn golden_scheme_evm_eip3009_payload_parses() {
    let p: PaymentPayload = serde_json::from_str(SPEC_EIP3009_PAYLOAD).unwrap();
    assert_eq!(p.x402_version, 2);
    assert_eq!(p.accepted.scheme, "exact");
    assert_eq!(p.accepted.network, "eip155:84532");
    assert_eq!(p.accepted.amount, "10000");
    let extra = p.accepted.extra.as_ref().unwrap();
    assert_eq!(extra.asset_transfer_method.as_deref(), Some("eip3009"));

    let e = ExactEvmEip3009Payload::from_payment(&p).unwrap();
    assert_eq!(e.authorization.from, "0x857b06519E91e3A54538791bDbb0E22373e36b66");
    assert_eq!(e.authorization.value, "10000");
    assert_eq!(
        e.authorization.nonce,
        "0xf3746613c2d920b5fdabc0856f2aeb2d4f88ee6037b8cc5d04a71a4462f13480"
    );
}

#[test]
fn golden_transport_payment_signature_header_decodes() {
    let p: PaymentPayload = decode_header(SPEC_PAYMENT_SIGNATURE_B64).unwrap();
    assert_eq!(p.x402_version, 2);
    assert_eq!(p.accepted.scheme, "exact");
    // Canonical transport `extra` carries no assetTransferMethod.
    let extra = p.accepted.extra.as_ref().unwrap();
    assert_eq!(extra.asset_transfer_method, None);
    assert_eq!(extra.name.as_deref(), Some("USDC"));
    let e = ExactEvmEip3009Payload::from_payment(&p).unwrap();
    assert_eq!(e.authorization.value, "10000");
}

#[test]
fn golden_transport_payment_response_success_decodes() {
    let s: SettlementResponse = decode_header(SPEC_PAYMENT_RESPONSE_OK_B64).unwrap();
    assert!(s.success);
    assert_eq!(s.network, "eip155:84532");
    assert_eq!(s.payer, "0x857b06519E91e3A54538791bDbb0E22373e36b66");
    assert!(s.transaction.starts_with("0x1234"));
    assert_eq!(s.error_reason, None);
}

#[test]
fn golden_transport_payment_response_failure_decodes() {
    let s: SettlementResponse = decode_header(SPEC_PAYMENT_RESPONSE_FAIL_B64).unwrap();
    assert!(!s.success);
    assert_eq!(s.error_reason.as_deref(), Some("insufficient_funds"));
    assert_eq!(s.transaction, "");
}

#[test]
fn golden_transport_payment_required_parses() {
    let r: PaymentRequired = serde_json::from_str(SPEC_PAYMENT_REQUIRED).unwrap();
    assert_eq!(r.x402_version, 2);
    assert_eq!(r.error.as_deref(), Some("PAYMENT-SIGNATURE header is required"));
    assert_eq!(r.accepts.len(), 1);
    assert_eq!(r.accepts[0].extra.as_ref().unwrap().asset_transfer_method, None);
}

#[test]
fn header_base64_roundtrip_preserves_payload() {
    let p: PaymentPayload = serde_json::from_str(SPEC_EIP3009_PAYLOAD).unwrap();
    let header = encode_header(&p).unwrap();
    let back: PaymentPayload = decode_header(&header).unwrap();
    assert_eq!(p, back);
}

#[test]
fn version_mismatch_is_rejected() {
    let mut p: PaymentPayload = serde_json::from_str(SPEC_EIP3009_PAYLOAD).unwrap();
    p.x402_version = 1;
    assert!(ExactEvmEip3009Payload::from_payment(&p).is_err());
}

proptest! {
    #[test]
    fn eip3009_authorization_json_roundtrip(
        from in "0x[0-9a-fA-F]{40}",
        value in "[0-9]{1,30}",
        nonce in "0x[0-9a-fA-F]{64}",
        valid_after in "[0-9]{1,12}",
        valid_before in "[0-9]{1,12}",
    ) {
        let a = Eip3009Authorization {
            from: from.clone(),
            to: from,
            value,
            valid_after,
            valid_before,
            nonce,
        };
        let s = serde_json::to_string(&a).unwrap();
        let b: Eip3009Authorization = serde_json::from_str(&s).unwrap();
        prop_assert_eq!(a, b);
    }
}
