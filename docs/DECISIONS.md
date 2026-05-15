# Technical decisions

Format: decision, context, consequence.

## D1 — axum over pingora

Context: the router fully transforms payloads (API format conversion),
plugs a routing engine into external stores, and stacks middleware
(multi-tenant auth, rate limit, circuit breaker, observability, hot
reload). pingora is optimized for high-throughput reverse-proxying with
little transformation; that is not the profile.

Decision: axum. The framework overhead gap is on the order of a
microsecond, drowned in the upstream LLM latency (hundreds of ms). The
gap in middleware implementation cost (the tower ecosystem vs pingora's
phase model) is measured in weeks over 12 months.

Consequence: standard tower / hyper / reqwest stack. Consistent with an
earlier Rust proxy partially reused (config, telemetry, error patterns).

## D2 — Two artifacts: `x402` crate + product binary

Context: the reuse and reference value is in the payment protocol
implementation, not in the router.

Decision: an `x402` crate independent of the LLM domain, publishable; a
`poc-x402-llm` binary that consumes it. Strict boundary.

Consequence: the crate carries no product opinion; real dogfooding from
the start.

## D3 — Pluggable payment-scheme abstraction

Context: x402 is not the only emerging agentic payment protocol.

Decision: the crate core exposes a scheme abstraction; x402 (`exact`)
is the first implementation, and the architecture survives the addition
of another scheme.

Consequence: de-risks dependency on a single standard. Cost: an
indirection layer to design correctly from the start.

## D4 — Facilitator-settled model, hosted facilitator for the POC

Context: x402 lets the server avoid touching the chain by delegating
verification and settlement to a facilitator.

Decision: POC on a hosted facilitator, JSON dialogue over reqwest, no
dedicated crate — exact control over the protocol, young ecosystem.

Consequence: no wallet or RPC node management on the server side for the
POC. Operational dependency on the facilitator to be documented as a
risk.

## D5 — Test network

Decision: Base Sepolia (testnet) for the POC. Free test funds, no real
monetary cost.

Consequence: a test recipient wallet to provide in local
configuration.

## D6 — Provider endpoints and secrets out of source

Decision: upstream hostname, model identifier, API keys and recipient
address live in untracked local configuration or environment variables,
never in versioned source.

Consequence: source and versioned tree stay neutral and portable. An
example configuration file, without real values, documents the expected
keys.

## D7 — Transport-agnostic core crate

Context: x402 has a V2. The headers (`PAYMENT-REQUIRED` /
`PAYMENT-SIGNATURE` / `PAYMENT-RESPONSE`) carry Base64 JSON; the network
is CAIP-2; `exact`/EVM relies on EIP-3009. The facilitator contract
reduces to two POSTs (`/verify`, `/settle`).

Decision: the `x402` crate depends on no HTTP client. It exposes the
wire types and a `FacilitatorTransport` trait; the concrete
implementation (reqwest) lives in the binary. The scaffold's
speculative `Scheme` trait is removed (no real second scheme —
pluggability kept as an architectural intent, not a premature
abstraction).

Consequence: minimal crate, no HTTP ecosystem lock-in.

Correctness boundary — **resolved**. Schemas aligned with the canonical
sources: `specs/transports-v2/http.md` (transport, headers,
`SettlementResponse`) and `go/FACILITATOR.md` (`/verify` `/settle`
API). The resolution fixed three errors in the provisional types: (1)
`extra.assetTransferMethod` is absent from the canonical
`PaymentRequired` → made optional; (2) missing top-level `error` field
on `PaymentRequired`; (3) `SettlementResponse` modeled wrong (missing
`payer`, wrong types) → aligned (`success`, `transaction`, `network`,
`payer`, `errorReason`). The facilitator request is `{paymentPayload,
paymentRequirements}` with no version wrapper. Golden tests pinned to
the verbatim Base64 from the spec.

## D8 — Protocol target: x402 V2 only

Decision: the crate implements V2 (`x402Version: 2`). V1 (`X-PAYMENT`
headers, free-form chain strings like `base-sepolia`) is not supported.

Consequence: a non-V2 payload is rejected explicitly
(`UnsupportedVersion`). No V1 compatibility path until justified by a
real need.

## D9 — POC upstream is an OpenAI-compatible passthrough

Context: the chosen POC upstream exposes an OpenAI-compatible
chat-completions API (`messages[]` with the system role inside the
array, `choices[].message.content`, `usage.prompt_tokens`). An earlier
adapter draft converted to a non-OpenAI Messages shape, which would
silently corrupt requests/responses against this upstream.

Decision: the POC adapter is a near-passthrough — forward the inbound
OpenAI body unchanged, only overriding `model` and defaulting
`max_tokens`, and relay the upstream response body verbatim. The
concrete endpoint/model live in untracked local config (D6); source
and docs stay provider-neutral.

Consequence: correct and minimal for an OpenAI-compatible upstream. A
Messages-style provider becomes a separate adapter module when needed
(D3), not a forced conversion.

## D10 — Settlement must leave the hot path (measured)

Context: a live Base Sepolia run (n=20, DeepSeek upstream, hosted
facilitator) measured per-request overhead. Medians: verify 228 ms,
settle 593 ms (p99 2229 ms), overhead 846 ms (p99 2439 ms), total
1693 ms. The architecture target is p99 overhead < 5 ms — missed by
~490×.

Finding: the Rust proxy's own cost is negligible. The overhead is
`verify` + `settle`, two synchronous facilitator round trips on the
hot path; `settle` blocks on chain confirmation.

Decision: synchronous per-request settlement is rejected as a viable
design. Settlement must move off the hot path — deferred/async, or the
x402 `batch-settlement` scheme. `verify` must be optimized (local or
optimistic verification, caching of recent payers). The settlement
mode is a structuring V1 decision, not out-of-MVP-scope as the
original brief assumed.

Consequence: the next POC iteration measures (a) verify-on-hot-path +
async settle, and (b) `batch-settlement`, against the same target.
Evidence: `README.md` Results, run captured at n=20.

## D11 — The x402 payment mode is a latency/risk dial (measured)

Context: POC-2 measured three points on Base Sepolia (n=20, same
harness, DeepSeek upstream). Overhead p99: baseline (sync settle,
facilitator verify) 2439 ms; async settle + facilitator verify 956 ms;
async settle + per-payer cached verify 0.141 ms. The pure proxy cost
is ~67 µs p50.

Finding: the < 5 ms overhead target is unreachable with strict x402
(~490× over). It is reachable (~35× under) only by (1) moving
settlement off the hot path and (2) caching verification per payer for
a short TTL. Each step buys latency with a named economic risk:
serve-before-settle, and a per-payer trust window. On-chain evidence:
async settlements landed but not all (42/≈46) — a `tokio::spawn` is
not durable.

Decision: V1 must treat the payment mode as an explicit, configurable
latency/risk dial, not a fixed choice. Required V1 building blocks:
a durable settlement queue with reconciliation (not fire-and-forget),
and bounded per-payer verify caching paired with per-payer spend caps.
Strict synchronous x402 is acceptable only for high-value, low-rate
calls; agent-grade per-call LLM traffic requires the relaxed modes
with their risk controls.

Consequence: POC-3 evaluates the x402 `batch-settlement` scheme as the
principled alternative to per-request settle. It is deliberately not
folded into POC-2: it is a distinct x402 scheme (separate payload and
facilitator interaction) and depends on hosted-facilitator support for
`batch-settlement` on Base Sepolia, which must be confirmed first.
Evidence: `README.md` Results POC-2, run logs at n=20.

## D12 — POC-3: Rust batch-settlement crypto core proven; lifecycle gap found

Context: POC-3 set out to measure the batch-settlement steady-state
hot path (local voucher verification, no facilitator, no chain).

Delivered and proven: a Rust `x402::batch_settlement` core —
EIP-712 domain/`channelId`/voucher digest + secp256k1 recover —
**byte-exact against viem** (the library the x402 client SDK signs
with), 5 golden tests pinned to authoritative vectors. No Rust
implementation of batch-settlement existed publicly; this one is
correct by demonstration, not assertion. This stands independent of
the result below.

Tranche-1 scoping error (found, then fixed in Tranche 1.5): deposit
settlement was deferred as a "Tranche 2 stub", but without the
facilitator `/settle` of the deposit the channel is never funded
on-chain and the server never returns a `PAYMENT-RESPONSE` confirming
the channel, so the client re-sent a deposit every request. Fix:
deposit branch now calls `/verify` then `/settle` (gasless,
facilitator-sponsored, one-off) and returns a spec-shaped
`PAYMENT-RESPONSE` (deposit and voucher variants); the client then
transitions to vouchers.

Result (Base Sepolia, n=20 pure vouchers): one on-chain deposit
(payer −0.1 USDC, escrow +0.1), then 20 voucher requests, recipient
unchanged (no per-request settlement). Voucher overhead — **release:
median 0.21 ms, p95 0.38 ms, p99 0.39 ms**; debug: 3.47 / 5.52 /
5.59 ms. settle 0; no facilitator round trip on the hot path. The
overhead is purely local secp256k1 recover + keccak EIP-712.

Decision: batch-settlement is the principled answer to D10/D11.
It clears the 5 ms p99 target by ~13× (release, p99 0.39 ms) with
**no risk compromise** — the per-request commitment is a
capital-backed EIP-712 voucher against an on-chain escrow, settlement
batched off the hot path — unlike the POC-2 latency/risk dial. Open
for production (Tranche 2, not blocking the measurement): the batched
claim/`settle` that moves escrow → recipient (receiver-authorizer
delegated to the facilitator).

Evidence: `README.md` Results POC-3, run-batch logs, on-chain
escrow/payer deltas.

### Tranche 2 outcome (bounded channel-manager core)

Scope decision (settled, session 2): the credential differentiator is
the channel-manager **correctness core** in Rust, not the voucher
primitive alone nor a feature-complete port. Built and unit-proven:
server-authored `ClaimPayload`/`SettlePayload`/`VoucherClaim` types
with golden serialization tests (9 batch tests total); a router core
that aggregates every channel's latest signed voucher into one claim,
is idempotent (`charged > total_claimed` gate, `total_claimed`
reconciled to the claimed ceiling on success — no double-claim),
ordered claim→settle, receiver-authorizer delegated to the
facilitator (no server signature); a thin periodic policy
(`runtime.claim_interval_secs`) plus a deterministic
`POST /admin/claim-settle` trigger. Hot path re-confirmed unchanged
(release voucher overhead p99 378 µs).

Honest status: the **claim leg executed on-chain successfully** (real
tx `0x84d607d1…`), proving the server-authored claim envelope is
correct and accepted by the hosted facilitator. The full
escrow → recipient closure was **not** cleanly demonstrated in one
run window: a settle returned empty once, and a fresh server process
hitting a channel already claimed on-chain produced "nothing to
claim". Both are recovery-after-state-loss / channel-reuse behaviors
**explicitly out of scope** by the bounded-scope decision above —
documented as a deliberate boundary, not hidden as missing work.
RECIPIENT balance did not visibly move; no closed-loop figure is
claimed. Closing it requires the excluded long-tail
(state reconciliation across restarts, settle retry), which is
operational/SDK territory, not the differentiating Rust core.

Evidence: claim tx hash above, run-batch/server-batch logs, on-chain
escrow unchanged after settle leg.
