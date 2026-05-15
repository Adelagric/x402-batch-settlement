//! Golden tests for the batch-settlement EVM crypto, pinned to
//! authoritative vectors produced with viem (the same library the
//! x402 client SDK signs with). A single wrong byte in any EIP-712
//! type string, the domain, or the encoding changes channelId or the
//! digest, so these vectors transitively prove the typehashes too.

use x402::batch_settlement::{
    recover_signer, verify_voucher, voucher_digest, BatchVoucherPayload, ClaimPayload,
    SettlePayload, VoucherClaim, VoucherClaimInner, WireChannelConfig, WireVoucher,
};

const CHAIN_ID: u64 = 84532;
const PAYER: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const RECEIVER: &str = "0x19ee5100D3a1e687F85B952bd3FbEc108Ab6A8d7";
const RECEIVER_AUTHORIZER: &str = "0xd407e409E34E0b9afb99EcCeb609bDbcD5e7f1bf";
const TOKEN: &str = "0x036CbD53842c5426634e7929541eC2318f3dCF7e";
const SALT: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";
const EXPECTED_CHANNEL_ID: &str =
    "0x5fafb915f0dbee350d7f84d91802dea47e8e3a71929c3cd79da161c291fb28bd";
const EXPECTED_VOUCHER_DIGEST: &str =
    "0xa2874adbecca0abb1884b4ac1c100e3906d25208ad0c9e6a8fcf9790ccfa2246";
const SIGNATURE: &str = "0x6ad7a9c0cd0172b09704c56dd22de6d2877cf912de007dcdc7757a68756b84af2d2767327e6b84836fe2b48fd1b09675957c2fa4ec6c33a146dddd0e823cf1341c";
const MAX_CLAIMABLE: &str = "1000";

fn unhex(s: &str) -> Vec<u8> {
    let s = s.strip_prefix("0x").unwrap();
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

fn wire_config() -> WireChannelConfig {
    WireChannelConfig {
        payer: PAYER.into(),
        payer_authorizer: PAYER.into(),
        receiver: RECEIVER.into(),
        receiver_authorizer: RECEIVER_AUTHORIZER.into(),
        token: TOKEN.into(),
        withdraw_delay: 900,
        salt: SALT.into(),
    }
}

#[test]
fn channel_id_matches_viem_reference() {
    let cfg = wire_config().parse().unwrap();
    assert_eq!(cfg.channel_id(CHAIN_ID).to_vec(), unhex(EXPECTED_CHANNEL_ID));
}

#[test]
fn voucher_digest_matches_viem_reference() {
    let cfg = wire_config().parse().unwrap();
    let cid = cfg.channel_id(CHAIN_ID);
    let d = voucher_digest(&cid, MAX_CLAIMABLE.parse().unwrap(), CHAIN_ID);
    assert_eq!(d.to_vec(), unhex(EXPECTED_VOUCHER_DIGEST));
}

#[test]
fn recover_signer_matches_payer_authorizer() {
    let digest: [u8; 32] = unhex(EXPECTED_VOUCHER_DIGEST).try_into().unwrap();
    let sig: [u8; 65] = unhex(SIGNATURE).try_into().unwrap();
    let signer = recover_signer(&digest, &sig).unwrap();
    assert_eq!(signer.to_vec(), unhex(PAYER));
}

#[test]
fn verify_voucher_end_to_end() {
    let payload = BatchVoucherPayload {
        kind: "voucher".into(),
        channel_config: wire_config(),
        voucher: WireVoucher {
            channel_id: EXPECTED_CHANNEL_ID.into(),
            max_claimable_amount: MAX_CLAIMABLE.into(),
            signature: SIGNATURE.into(),
        },
    };
    let (cid, max) = verify_voucher(&payload, CHAIN_ID).unwrap();
    assert_eq!(cid.to_vec(), unhex(EXPECTED_CHANNEL_ID));
    assert_eq!(max, 1000);
}

#[test]
fn claim_payload_serializes_to_spec_shape() {
    let claim = ClaimPayload::new(vec![VoucherClaim {
        voucher: VoucherClaimInner {
            channel: wire_config(),
            max_claimable_amount: "5000".into(),
        },
        signature: SIGNATURE.into(),
        total_claimed: "5000".into(),
    }]);
    let v = serde_json::to_value(&claim).unwrap();
    assert_eq!(v["type"], "claim");
    let c = &v["claims"][0];
    assert_eq!(c["voucher"]["maxClaimableAmount"], "5000");
    assert_eq!(c["voucher"]["channel"]["payerAuthorizer"], PAYER);
    assert_eq!(c["voucher"]["channel"]["receiverAuthorizer"], RECEIVER_AUTHORIZER);
    assert_eq!(c["voucher"]["channel"]["withdrawDelay"], 900);
    assert_eq!(c["signature"], SIGNATURE);
    assert_eq!(c["totalClaimed"], "5000");
    // claimAuthorizerSignature must be absent (delegated to facilitator).
    assert!(claim.claims[0].voucher.channel.salt.starts_with("0x"));
    assert!(v.get("claimAuthorizerSignature").is_none());
}

#[test]
fn settle_payload_serializes_to_spec_shape() {
    let s = SettlePayload::new(RECEIVER, TOKEN);
    let v = serde_json::to_value(&s).unwrap();
    assert_eq!(v["type"], "settle");
    assert_eq!(v["receiver"], RECEIVER);
    assert_eq!(v["token"], TOKEN);
}

#[test]
fn verify_voucher_rejects_tampered_amount() {
    let payload = BatchVoucherPayload {
        kind: "voucher".into(),
        channel_config: wire_config(),
        voucher: WireVoucher {
            channel_id: EXPECTED_CHANNEL_ID.into(),
            max_claimable_amount: "999".into(), // signature was over 1000
            signature: SIGNATURE.into(),
        },
    };
    assert!(verify_voucher(&payload, CHAIN_ID).is_err());
}
