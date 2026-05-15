# x402 — batch-settlement core (Rust)

A Rust crate covering x402 **V2** wire types and the **batch-settlement**
EVM cryptographic core: EIP-712 `ChannelConfig → channelId`, `Voucher`
digest, secp256k1 signer recovery, and the server-authored
`claim` / `settle` payloads.

## Where this sits

The mature, general Rust x402 implementation is
[`x402-rs`](https://github.com/x402-rs/x402-rs) (`exact` scheme, V2,
facilitator, axum/reqwest). This crate is **not** a competing general
implementation. Its focus is the **`batch-settlement` scheme**, which
the Rust ecosystem does not yet implement (it is "Planned / Deferred"
on the `x402-rs` roadmap, and absent from `x402-core`). It exists to
back a measured study of payment-gated LLM serving (see the repository
`docs/`) and as a reference / contribution candidate for
batch-settlement in Rust.

## Scope

Implemented and tested:

- x402 **V2** wire types: `PaymentRequired`, `PaymentPayload`,
  `SettlementResponse`, the three Base64 headers
- `exact` scheme on EVM (EIP-3009) — used by the study's baseline
- **`batch-settlement` cryptographic core**: EIP-712
  `ChannelConfig → channelId`, `Voucher` digest, secp256k1 recovery,
  server-authored `claim` / `settle`
- transport-agnostic facilitator contract (no HTTP-client dependency)

Correctness is pinned to the canonical spec and **byte-exact against
`viem`** (the library the official client SDK signs with): golden
tests decode the verbatim Base64 vectors from the x402 spec and
reproduce `channelId`, the voucher digest and signer recovery exactly.

Deliberately out of scope (a bounded surface, not missing work): the
batch-settlement channel-lifecycle long-tail — cooperative refund,
recovery-after-state-loss, withdrawal monitoring, multi-policy claim
strategies.

## Use

```rust
use x402::batch_settlement::verify_voucher;

// Recovers the EIP-712 voucher signer and checks it is the channel's
// payerAuthorizer. chain_id 84532 = Base Sepolia. No facilitator,
// no chain.
let (channel_id, max_claimable) = verify_voucher(&payload, 84532)?;
```

## Status

Pre-1.0, experimental. V2 only. No `unsafe`. Licensed MIT OR Apache-2.0.
