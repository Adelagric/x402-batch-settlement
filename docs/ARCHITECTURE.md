# Architecture

## Purpose

A cost-aware router for LLM APIs. A single entry layer, compatible with
the OpenAI API format, that routes requests to multiple LLM providers
based on cost, latency and availability, with per-request programmatic
payment gating (the x402 protocol).

## Two-artifact split

1. **`x402`** (crate, independent of the LLM domain) — a Rust
   implementation of the x402 payment protocol: payment construction
   and verification, facilitator client, scheme types, pluggable scheme
   abstraction. No dependency on LLM routing.
2. **`poc-x402-llm`** (binary) — the router. Consumes the `x402` crate
   to gate access, converts the inbound OpenAI format to the upstream
   provider format, forwards, and converts the response back.

Strict boundary: the crate does not know about LLMs, the binary does
not reimplement x402.

## Target components (beyond the POC)

- Per-provider adapters (Messages-style and OpenAI-style APIs,
  commercial and open)
- Pluggable routing engine (round-robin, cheapest, fastest, weighted)
- Per-request cost tracking with fine-grained attribution
- Failover with a circuit breaker
- Observability: Prometheus + OpenTelemetry
- Multi-tenant (per-client API keys)
- Hot configuration reload without dropping connections

## Stack

- Rust stable
- axum, tokio, tower
- reqwest for outbound calls (facilitator + upstream provider)
- sqlx + PostgreSQL, Redis for rate limiting and real-time cost
  tracking (post-POC)
- TOML configuration; provider endpoints and secrets in untracked
  local configuration or environment variables

## Quality constraints

- No `unsafe` without a written justification in the source
- Property tests on the routing paths and on payment-payload
  encoding/decoding
- Concurrency checking (loom) on the circuit breaker
- Benchmarks (criterion) on the hot paths
- p99 overhead < 5 ms per routed request
- OpenAPI specification kept up to date

## Out of POC scope

- Management UI (a CLI is enough)
- Response caching
- Semantic routing
- SSE streaming: the per-request-payment + streaming coupling is
  handled in a dedicated POC
