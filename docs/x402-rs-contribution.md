# Proposal: `batch-settlement` (V2, EVM) for x402-rs

Draft for an issue on `x402-rs/x402-rs`. Review and put in your own
voice before posting.

## Context

`x402-rs` is the mature general Rust x402 implementation: `exact` V2
across chains, a facilitator, axum/reqwest middleware. The roadmap
lists the deferred / `batch-settlement` family as **Planned**. It is
also absent from `x402-core`. So there is currently no Rust
implementation of `batch-settlement`.

While measuring the latency cost of x402-gated LLM serving, I built a
spec-pinned `batch-settlement` (V2, EVM) cryptographic core and would
like to contribute it, aligned to your scheme architecture.

## Why batch-settlement matters here (motivation, measured)

Same harness, Base Sepolia, overhead = total − upstream:

- `exact`, synchronous verify+settle: p99 overhead ≈ 2.4 s — ~490×
  over a 5 ms target; the per-request facilitator round trips and
  on-chain settle dominate.
- `batch-settlement`, steady-state voucher (release build): p99
  overhead ≈ 0.39 ms — no facilitator round trip, no chain on the hot
  path, value committed once on-chain via the channel deposit.

For agent / pay-per-call workloads this is the difference between
unusable and usable. It is the strongest argument for prioritising the
scheme.

## What I have, precisely

- EIP-712 `ChannelConfig → channelId`, `Voucher` digest, secp256k1
  signer recovery, and the server-authored `claim` / `settle`
  payloads.
- **Golden tests byte-exact against `viem`** (the library the official
  client SDK signs with): the verbatim Base64 vectors from the x402
  spec decode and reproduce `channelId`, the voucher digest and signer
  recovery exactly. These vectors are reusable as conformance tests
  regardless of how the scheme is structured here.
- A reference router that exercises the hot path; the `claim` leg has
  executed on-chain against the hosted facilitator.

## How it could fit x402-rs

Issue-first, to align with your design before any PR:

- A `v2-eip155-batch-settlement` handler implementing
  `X402SchemeFacilitator` (verify/settle/supported) in
  `x402-chain-eip155`, registered via `SchemeBlueprints`.
- The client side via `X402SchemeClient` / `PaymentCandidate`.
- The EIP-712 vectors contributed as conformance tests in
  `x402-types` or the chain crate.

I would adapt the core to your `X402SchemeId` /
`X402SchemeFacilitatorBuilder` model rather than drop in a parallel
abstraction.

## Honest scope / maturity

In: the cryptographic core, voucher verification, server-authored
`claim`/`settle`. Proven by golden vectors and an on-chain `claim`.

Not done: the channel-lifecycle long-tail — cooperative refund,
recovery-after-state-loss, withdrawal monitoring, multi-policy claim
strategies. The full escrow → recipient closure was demonstrated only
partially (the `claim` leg landed on-chain; the `settle` leg and
cross-restart state reconciliation are not fully closed). I am
flagging this so the maturity is clear up front.

Reference and vectors: <link your repository here after renaming it to
avoid the name clash with the `x402-rs` org>.
