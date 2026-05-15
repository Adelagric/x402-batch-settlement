//! `batch-settlement` scheme, EVM binding — cryptographic core.
//!
//! Stateless unidirectional payment channels: the client signs a
//! cumulative EIP-712 `Voucher` per request; the server verifies it
//! locally (no facilitator, no chain) against the channel's
//! `payerAuthorizer`. This module is the protocol crypto only — channel
//! state, claim and settle are the consumer's concern.
//!
//! Pinned from the x402 reference (constants.ts / utils.ts,
//! commit-current): domain `x402 Batch Settlement` v`1`, bound to
//! `chainId` + the `x402BatchSettlement` contract; `channelId =
//! EIP712(ChannelConfig)`; voucher = `Voucher(bytes32 channelId,
//! uint128 maxClaimableAmount)`.

use serde::{Deserialize, Serialize};
use tiny_keccak::{Hasher, Keccak};

use crate::error::Error;

pub const SCHEME_BATCH_SETTLEMENT: &str = "batch-settlement";

/// Canonical CREATE2 address of the `x402BatchSettlement` contract
/// (same on every supported EVM chain), used as EIP-712
/// `verifyingContract`.
pub const BATCH_SETTLEMENT_CONTRACT: [u8; 20] = hexlit(b"4020074e9dF2ce1deE5A9C1b5c3f541D02a10003");

const DOMAIN_NAME: &[u8] = b"x402 Batch Settlement";
const DOMAIN_VERSION: &[u8] = b"1";
const EIP712_DOMAIN_TYPE: &[u8] =
    b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";
const CHANNEL_CONFIG_TYPE: &[u8] = b"ChannelConfig(address payer,address payerAuthorizer,address receiver,address receiverAuthorizer,address token,uint40 withdrawDelay,bytes32 salt)";
const VOUCHER_TYPE: &[u8] = b"Voucher(bytes32 channelId,uint128 maxClaimableAmount)";

fn keccak256(data: &[u8]) -> [u8; 32] {
    let mut k = Keccak::v256();
    k.update(data);
    let mut out = [0u8; 32];
    k.finalize(&mut out);
    out
}

/// Left-pad a 20-byte address into a 32-byte EIP-712 word.
fn word_addr(a: &[u8; 20]) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a);
    w
}

/// Big-endian 32-byte word from a u128 (covers uint40/uint128/uint256).
fn word_u128(v: u128) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[16..].copy_from_slice(&v.to_be_bytes());
    w
}

/// Immutable channel configuration. `channel_id` is its EIP-712 hash.
#[derive(Debug, Clone)]
pub struct ChannelConfig {
    pub payer: [u8; 20],
    pub payer_authorizer: [u8; 20],
    pub receiver: [u8; 20],
    pub receiver_authorizer: [u8; 20],
    pub token: [u8; 20],
    pub withdraw_delay: u64,
    pub salt: [u8; 32],
}

fn domain_separator(chain_id: u64, verifying_contract: &[u8; 20]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(160);
    buf.extend_from_slice(&keccak256(EIP712_DOMAIN_TYPE));
    buf.extend_from_slice(&keccak256(DOMAIN_NAME));
    buf.extend_from_slice(&keccak256(DOMAIN_VERSION));
    buf.extend_from_slice(&word_u128(chain_id as u128));
    buf.extend_from_slice(&word_addr(verifying_contract));
    keccak256(&buf)
}

fn eip712_digest(domain_sep: &[u8; 32], struct_hash: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(66);
    buf.extend_from_slice(&[0x19, 0x01]);
    buf.extend_from_slice(domain_sep);
    buf.extend_from_slice(struct_hash);
    keccak256(&buf)
}

impl ChannelConfig {
    /// EIP-712 `channelId = hashTypedData(ChannelConfig)` bound to the
    /// chain and the `x402BatchSettlement` contract.
    pub fn channel_id(&self, chain_id: u64) -> [u8; 32] {
        let mut s = Vec::with_capacity(256);
        s.extend_from_slice(&keccak256(CHANNEL_CONFIG_TYPE));
        s.extend_from_slice(&word_addr(&self.payer));
        s.extend_from_slice(&word_addr(&self.payer_authorizer));
        s.extend_from_slice(&word_addr(&self.receiver));
        s.extend_from_slice(&word_addr(&self.receiver_authorizer));
        s.extend_from_slice(&word_addr(&self.token));
        s.extend_from_slice(&word_u128(self.withdraw_delay as u128));
        s.extend_from_slice(&self.salt);
        let struct_hash = keccak256(&s);
        let sep = domain_separator(chain_id, &BATCH_SETTLEMENT_CONTRACT);
        eip712_digest(&sep, &struct_hash)
    }
}

/// EIP-712 digest of `Voucher(channelId, maxClaimableAmount)`.
pub fn voucher_digest(channel_id: &[u8; 32], max_claimable: u128, chain_id: u64) -> [u8; 32] {
    let mut s = Vec::with_capacity(96);
    s.extend_from_slice(&keccak256(VOUCHER_TYPE));
    s.extend_from_slice(channel_id);
    s.extend_from_slice(&word_u128(max_claimable));
    let struct_hash = keccak256(&s);
    let sep = domain_separator(chain_id, &BATCH_SETTLEMENT_CONTRACT);
    eip712_digest(&sep, &struct_hash)
}

/// Recover the Ethereum address that produced `signature` over the
/// 32-byte `digest`. `signature` is 65 bytes (r‖s‖v, v ∈ {0,1,27,28}).
pub fn recover_signer(digest: &[u8; 32], signature: &[u8; 65]) -> Result<[u8; 20], Error> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};

    let sig = Signature::from_slice(&signature[..64])
        .map_err(|e| Error::BatchSettlement(format!("bad signature: {e}")))?;
    let v = signature[64];
    let v = if v >= 27 { v - 27 } else { v };
    let rid = RecoveryId::from_byte(v)
        .ok_or_else(|| Error::BatchSettlement(format!("bad recovery id {v}")))?;
    let vk = VerifyingKey::recover_from_prehash(digest, &sig, rid)
        .map_err(|e| Error::BatchSettlement(format!("recover failed: {e}")))?;
    let pt = vk.to_encoded_point(false);
    let hash = keccak256(&pt.as_bytes()[1..]);
    let mut addr = [0u8; 20];
    addr.copy_from_slice(&hash[12..]);
    Ok(addr)
}

// ---- wire payload (subset: the `voucher` type) ----

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireChannelConfig {
    pub payer: String,
    pub payer_authorizer: String,
    pub receiver: String,
    pub receiver_authorizer: String,
    pub token: String,
    pub withdraw_delay: u64,
    pub salt: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WireVoucher {
    pub channel_id: String,
    pub max_claimable_amount: String,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchVoucherPayload {
    #[serde(rename = "type")]
    pub kind: String,
    pub channel_config: WireChannelConfig,
    pub voucher: WireVoucher,
}

impl WireChannelConfig {
    pub fn parse(&self) -> Result<ChannelConfig, Error> {
        Ok(ChannelConfig {
            payer: addr(&self.payer)?,
            payer_authorizer: addr(&self.payer_authorizer)?,
            receiver: addr(&self.receiver)?,
            receiver_authorizer: addr(&self.receiver_authorizer)?,
            token: addr(&self.token)?,
            withdraw_delay: self.withdraw_delay,
            salt: b32(&self.salt)?,
        })
    }
}

/// Cryptographically verify a `voucher` payload: recompute `channelId`,
/// check it matches the signed voucher, and confirm the EIP-712
/// signature recovers to the channel's `payerAuthorizer` (EOA path).
/// Returns `(channel_id, max_claimable)` on success. Channel balance
/// and cumulative-amount policy are the caller's responsibility.
pub fn verify_voucher(
    payload: &BatchVoucherPayload,
    chain_id: u64,
) -> Result<([u8; 32], u128), Error> {
    let cfg = payload.channel_config.parse()?;
    if cfg.payer_authorizer == [0u8; 20] {
        return Err(Error::BatchSettlement(
            "zero payerAuthorizer requires facilitator (EIP-1271)".into(),
        ));
    }
    let computed = cfg.channel_id(chain_id);
    let claimed = b32(&payload.voucher.channel_id)?;
    if computed != claimed {
        return Err(Error::BatchSettlement("channelId mismatch".into()));
    }
    let max: u128 = payload
        .voucher
        .max_claimable_amount
        .parse()
        .map_err(|_| Error::BatchSettlement("bad maxClaimableAmount".into()))?;
    let digest = voucher_digest(&computed, max, chain_id);
    let sig = sig65(&payload.voucher.signature)?;
    let signer = recover_signer(&digest, &sig)?;
    if signer != cfg.payer_authorizer {
        return Err(Error::BatchSettlement(
            "voucher signer is not payerAuthorizer".into(),
        ));
    }
    Ok((computed, max))
}

// ---- server-authored claim / settle (off the hot path) ----
//
// Posted to the facilitator `/settle` as the `payload` of a
// PaymentPayload. Pinned from x402 `types.ts`
// (BatchSettlementVoucherClaim / ClaimPayload / SettlePayload) and
// channelManager.ts. `claimAuthorizerSignature` is omitted: the
// receiver authorizer is delegated to the facilitator, which signs.

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherClaimInner {
    pub channel: WireChannelConfig,
    pub max_claimable_amount: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherClaim {
    pub voucher: VoucherClaimInner,
    pub signature: String,
    pub total_claimed: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaimPayload {
    #[serde(rename = "type")]
    pub kind: String,
    pub claims: Vec<VoucherClaim>,
}

impl ClaimPayload {
    pub fn new(claims: Vec<VoucherClaim>) -> Self {
        Self {
            kind: "claim".to_string(),
            claims,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettlePayload {
    #[serde(rename = "type")]
    pub kind: String,
    pub receiver: String,
    pub token: String,
}

impl SettlePayload {
    pub fn new(receiver: impl Into<String>, token: impl Into<String>) -> Self {
        Self {
            kind: "settle".to_string(),
            receiver: receiver.into(),
            token: token.into(),
        }
    }
}

// ---- hex helpers ----

fn unhex(s: &str) -> Result<Vec<u8>, Error> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if !s.len().is_multiple_of(2) {
        return Err(Error::BatchSettlement("odd hex length".into()));
    }
    (0..s.len() / 2)
        .map(|i| {
            u8::from_str_radix(&s[2 * i..2 * i + 2], 16)
                .map_err(|_| Error::BatchSettlement("bad hex".into()))
        })
        .collect()
}

fn addr(s: &str) -> Result<[u8; 20], Error> {
    let v = unhex(s)?;
    if v.len() != 20 {
        return Err(Error::BatchSettlement("address must be 20 bytes".into()));
    }
    let mut a = [0u8; 20];
    a.copy_from_slice(&v);
    Ok(a)
}

fn b32(s: &str) -> Result<[u8; 32], Error> {
    let v = unhex(s)?;
    if v.len() != 32 {
        return Err(Error::BatchSettlement("expected 32 bytes".into()));
    }
    let mut a = [0u8; 32];
    a.copy_from_slice(&v);
    Ok(a)
}

fn sig65(s: &str) -> Result<[u8; 65], Error> {
    let v = unhex(s)?;
    if v.len() != 65 {
        return Err(Error::BatchSettlement("signature must be 65 bytes".into()));
    }
    let mut a = [0u8; 65];
    a.copy_from_slice(&v);
    Ok(a)
}

/// Compile-time hex literal (20-byte address) for the contract const.
const fn hexlit(h: &[u8; 40]) -> [u8; 20] {
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => 0,
        }
    }
    let mut out = [0u8; 20];
    let mut i = 0;
    while i < 20 {
        out[i] = (nib(h[2 * i]) << 4) | nib(h[2 * i + 1]);
        i += 1;
    }
    out
}
