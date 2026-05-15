# Implementing x402 in Rust: V2 wire format, EIP-712 pitfalls, and the latency/economic-risk frontier of payment-gated LLM serving

This documents a Rust implementation of the x402 V2 payment protocol
and a measured study of what it costs to put x402 in front of an
LLM API. All figures are from live runs on Base Sepolia against the
hosted facilitator and an OpenAI-compatible upstream. Numbers are
overhead = total request time − upstream time, n = 20 unless noted.

## Why

A mature general Rust x402 implementation already exists —
[`x402-rs`](https://github.com/x402-rs/x402-rs) (the `exact` scheme,
V2, a facilitator, axum/reqwest). This work does not duplicate it. Two
things were missing:

1. **A measured answer**: how much latency does x402 actually add to
   an LLM call, and what does each settlement mode cost? Nobody had
   published this.
2. **`batch-settlement` in Rust**: the scheme that removes per-request
   settlement from the hot path is not implemented in the Rust
   ecosystem (it is "Planned / Deferred" on the `x402-rs` roadmap and
   absent from `x402-core`). The study needed it, so it was built and
   pinned to the spec.

So this is a measurement study plus a focused, spec-pinned
batch-settlement core — not a general x402 library.

## EIP-712 pitfalls (verified against `viem`)

Two concrete traps an implementer hits. Both were resolved by pinning
to the canonical source and proven with golden tests that reproduce
`viem`'s output byte-for-byte.

1. **`channelId` is a full EIP-712 hash, not `keccak256(abi.encode)`.**
   The batch-settlement client `voucher.ts` docstring describes
   `channelId` as `keccak256(abi.encode(ChannelConfig))`. The
   authoritative `utils.ts::computeChannelId` uses
   `hashTypedData(...)` — i.e. `keccak256(0x1901 ‖ domainSeparator ‖
   hashStruct(ChannelConfig))`, bound to `chainId` and the
   `x402BatchSettlement` contract. An implementer trusting the
   docstring computes a wrong channel id and every voucher fails.

2. **`extra.assetTransferMethod` is not part of the canonical
   `PaymentRequired`.** The `exact`/EVM scheme examples include
   `assetTransferMethod` inside `extra`; the canonical V2 transport
   `PaymentRequired` carries only `{name, version}`. Modeling
   `assetTransferMethod` as required makes a server fail to parse
   real 402 challenges. It must be optional.

The crate's golden tests decode the verbatim Base64 `PAYMENT-SIGNATURE`
and `PAYMENT-RESPONSE` values from the spec and assert exact equality
of `channelId`, the `Voucher` digest, and recovered signer.

## The latency / economic-risk frontier

Same harness across all modes.

| mode | overhead p50 | p95 | p99 | risk added |
|------|-------------:|----:|----:|------------|
| `exact`, synchronous verify + settle | 846 ms | 1410 ms | 2439 ms | none |
| `exact`, async settle, facilitator verify | 242 ms | 413 ms | 956 ms | serve-before-settle |
| `exact`, async settle, per-payer cached verify | 0.067 ms | 0.117 ms | 0.141 ms | per-payer trust window |
| `batch-settlement`, voucher (release build) | 0.21 ms | 0.38 ms | 0.39 ms | none |

Reading:

- Synchronous per-request x402 (`exact`) misses a 5 ms p99 overhead
  target by ~490×. The proxy's own cost is ~67 µs; the overhead is
  entirely the two facilitator round trips, `settle` blocking on chain
  confirmation.
- You can buy the target back with `exact` by moving settlement
  off-path and caching verification — but each step is an explicit
  economic risk (serving before settlement; trusting a payer for a TTL
  window).
- `batch-settlement` reaches the target (p99 0.39 ms, release) **with
  no risk compromise**: every request carries a capital-backed
  EIP-712 voucher against an on-chain escrow, value is committed once
  on-chain, and settlement is batched off the hot path. This is the
  principled answer, not a latency/safety trade.

## What is built

A transport-agnostic Rust crate: V2 wire types, `exact`/EVM
(EIP-3009), and the `batch-settlement` cryptographic core (EIP-712
`channelId` and `Voucher`, secp256k1 recovery, server-authored
`claim`/`settle`), all golden-tested against `viem`. A reference
router consumes it; the `batch-settlement` claim leg has executed
on-chain.

## Scope boundary (deliberate)

The channel-lifecycle long-tail — cooperative refund,
recovery-after-state-loss, withdrawal monitoring, multi-policy claim
strategies — is intentionally out of scope. It is operational
plumbing the official SDKs already cover; reimplementing it in Rust
adds maintenance surface, not differentiation. The differentiating,
correctness-critical core is in; the boundary is a decision, not a
gap.
