# Cost-aware LLM API router — x402 POC

A single OpenAI-compatible entry point that gates each LLM request
behind a per-request programmatic payment (x402 V2), forwards to an
upstream provider, and reports its own overhead.

## Layout

| Path | Role |
|------|------|
| `crates/x402` | x402 V2 protocol crate, transport-agnostic, no LLM coupling |
| `poc-x402-llm` | the router binary, consumes the crate |
| `test-agent` | x402 V2 buyer that pays and measures overhead |
| `docs/` | architecture and decision records |

The crate is the reusable artifact; the binary is one consumer. The
boundary is strict (see `docs/DECISIONS.md`, D2/D7).

## Protocol correctness

The crate is pinned to the canonical x402 spec, not to memory. Wire
types and the facilitator contract are aligned with
`specs/transports-v2/http.md` and `go/FACILITATOR.md` of the x402
foundation repository. Golden tests decode the verbatim Base64 header
values from the spec. V2 only (`x402Version: 2`); V1 is rejected.

## Prerequisites

- Rust stable, Node 20+
- A Base Sepolia recipient wallet (receives test payments)
- A funded Base Sepolia buyer wallet for `test-agent`:
  - test ETH for gas headroom — Base / Coinbase Developer Platform faucet
  - test USDC for payments — Circle faucet
- An upstream provider endpoint, model id and API key

No real funds are involved: Base Sepolia is a testnet.

## Configuration

Copy `config.example.toml` to `config.local.toml` (gitignored) and fill
in real values. Any key can be overridden by environment:
`AR_<SECTION>__<KEY>`, e.g. `AR_UPSTREAM__API_KEY`. The upstream
endpoint, model id, API key and recipient address never enter the
versioned tree.

## Run

```sh
cargo run -p poc-x402-llm                       # serves on config.server.bind
cd test-agent && npm install
EVM_PRIVATE_KEY=0x... RUNS=20 npm start         # pays and measures
```

The agent issues a request, receives `402` with a `PAYMENT-REQUIRED`
challenge, constructs and signs a payment, retries with
`PAYMENT-SIGNATURE`, and on success reads the timing headers.

## Measurement

The router emits per-request timing headers in microseconds:

| Header | Meaning |
|--------|---------|
| `x-verify-us` | facilitator `/verify` round trip |
| `x-upstream-us` | upstream provider call |
| `x-settle-us` | facilitator `/settle` round trip |
| `x-overhead-us` | total minus upstream — the router's own cost |
| `x-total-us` | full handler |

`test-agent` reports median / p95 / p99 over the configured run count.
The target from `docs/ARCHITECTURE.md` is p99 overhead < 5 ms; the
overhead here additionally includes the payment round trips, which is
the honest figure for a payment-gated path.

## Results — POC-1: sync settle, facilitator verify (Base Sepolia, n=20)

Upstream: DeepSeek `deepseek-chat`. Facilitator: hosted x402.org.
All values in milliseconds.

| stage | median | p95 | p99 |
|-------|-------:|----:|----:|
| verify | 228 | 273 | 330 |
| upstream | 824 | 1050 | 1206 |
| settle | 593 | 1138 | 2229 |
| overhead (total − upstream) | 846 | 1410 | 2439 |
| total | 1693 | 2372 | 3645 |

The architecture target (`docs/ARCHITECTURE.md`) is p99 overhead
< 5 ms. Measured p99 overhead is ~2439 ms — about **490× over
target**. The Rust proxy's own cost (parsing, codec, dispatch) is
negligible; the overhead is entirely `verify` + `settle`, two
synchronous facilitator round trips on the hot path, with `settle`
blocking on Base Sepolia confirmation.

Conclusion: synchronous per-request settlement is not viable for a
latency-sensitive payment-gated LLM proxy. Settlement must leave the
hot path (deferred/async, or the x402 `batch-settlement` scheme), and
`verify` must be optimized (local/optimistic verification, caching).
The settlement mode is a structuring V1 decision, not a late add-on.
See `docs/DECISIONS.md`, D10.

## Results — POC-2: the latency/risk frontier (Base Sepolia, n=20)

Same harness, same upstream. Overhead = total − upstream. Three
points on the latency vs economic-risk curve:

| mode | overhead p50 | p95 | p99 | vs 5 ms target | risk added |
|------|-------------:|----:|----:|---------------:|------------|
| baseline (sync settle, facilitator verify) | 846 ms | 1410 ms | 2439 ms | ~490× over | none (strict) |
| async settle, facilitator verify | 242 ms | 413 ms | 956 ms | ~191× over | serve-before-settle |
| async settle, cached verify (TTL, per payer) | 0.067 ms | 0.117 ms | 0.141 ms | **~35× under** | + per-payer trust window |

On-chain: async settlements land (RECIPIENT +42 over the two runs),
but 42 of ~46 requested — confirming a `tokio::spawn` is not durable;
a real system needs a settlement queue with reconciliation.

Reading: with strict x402 (synchronous settle + verify) the 5 ms
target is missed by ~490×. The Rust proxy's own cost is ~67 µs (p50)
— it is not the problem. The target is reachable only by moving
settlement off the hot path **and** caching verification per payer,
i.e. by purchasing latency with two explicit economic risks:
serve-before-settle (needs a durable settlement queue + reconciliation)
and a per-payer trust window (needs per-payer spend caps + a short
TTL). The x402 payment mode is a latency/risk dial and a V1
architecture decision. See `docs/DECISIONS.md`, D11.

## Results — POC-3: batch-settlement (Base Sepolia, n=20 pure vouchers)

A Rust `x402::batch_settlement` core was implemented and **proven
correct**: EIP-712 domain / `channelId` / voucher digest and
secp256k1 recovery are byte-exact against viem (the library the x402
client SDK signs with), 5 golden tests pinned to authoritative
vectors. No public Rust implementation of batch-settlement existed.

End-to-end measured on Base Sepolia. One on-chain deposit in warmup
(payer −0.1 USDC, escrow +0.1 USDC), then 20 requests served against
off-chain cumulative vouchers. Recipient balance unchanged: vouchers
are never settled per request — value is committed once on-chain and
claimed/settled in batches off the hot path.

Voucher steady-state overhead (= total − upstream), settle is 0 by
construction:

| build | median | p95 | p99 |
|-------|-------:|----:|----:|
| release (target-cpu=native) | **0.21 ms** | 0.38 ms | **0.39 ms** |
| debug | 3.47 ms | 5.52 ms | 5.59 ms |

The overhead is entirely local secp256k1 recover + keccak EIP-712 +
response build — no facilitator round trip, no chain, no per-request
settlement. Debug inflates the crypto ~16×; release is the
representative figure.

Reading: batch-settlement clears the 5 ms p99 target by ~13×
(release) **with no risk compromise** — unlike POC-2 mode-3, which hit
0.14 ms only by accepting a per-payer trust window. Here every request
carries a capital-backed EIP-712 voucher against an on-chain escrow;
settlement is batched off the hot path. This is the principled
solution, not a latency/safety trade. Caveat: the batched
claim/settle that moves escrow → recipient is not implemented (the
facilitator holds the delegated receiver-authorizer); a production
deployment must run it. See `docs/DECISIONS.md`, D12.

**Tranche 2 (bounded channel-manager core).** Server-authored
claim/settle types + a router core (multi-channel aggregation,
idempotent claim→settle, facilitator-delegated authorizer) are
implemented and unit-proven (9 batch tests). The claim leg executed
on-chain (real tx). The full escrow → recipient closure was not
cleanly demonstrated in one run (settle empty once; cross-restart
channel-state reconciliation produced "nothing to claim") — these are
the recovery/state-reconciliation behaviors deliberately kept out of
scope. Hot path unchanged (release p99 378 µs). No closed-loop figure
is claimed; see `docs/DECISIONS.md`, D12 (Tranche 2 outcome).

## Documented failure modes

- No / invalid `PAYMENT-SIGNATURE` → `402` with a fresh challenge.
- Verification invalid → `402` with the facilitator reason.
- Payment verified but upstream fails → settlement is **skipped**; the
  client keeps its funds; the router returns `502`.
- Resource delivered but settlement fails → surfaced as an error; a
  production system would queue a settlement retry (out of POC scope).

## Known scope limits

Addresses, hex and signatures are validated only by JSON shape, not
strongly typed; local cryptographic verification of the EIP-3009
signature is not implemented (only needed for a facilitator-less mode).
Streaming is a separate POC: per-request payment plus SSE raises an
open question (charge per request or per token).
