// x402 V2 buyer. Pays the router per request and records the overhead
// headers it returns. Buyer wiring follows the V2 pattern from the
// x402 migration guide (specs: github.com/x402-foundation/x402).
//
// Resilient: a failed request is counted and the run continues; the
// summary reports successful samples plus a failure count, so one
// upstream blip never destroys a measurement.
//
// Env:
//   EVM_PRIVATE_KEY  test wallet private key (Base Sepolia, funded)
//   TARGET_URL       default http://127.0.0.1:8080/v1/chat/completions
//   RUNS             measured requests, default 20
//   WARMUP           warmup requests, default 3

import axios from "axios";
import { x402Client, wrapAxiosWithPayment } from "@x402/axios";
import { ExactEvmScheme } from "@x402/evm/exact/client";
import { privateKeyToAccount } from "viem/accounts";

const KEY = process.env.EVM_PRIVATE_KEY;
if (!KEY) {
  console.error("EVM_PRIVATE_KEY is required");
  process.exit(1);
}
const TARGET = process.env.TARGET_URL ?? "http://127.0.0.1:8080/v1/chat/completions";
const RUNS = Number(process.env.RUNS ?? 20);
const WARMUP = Number(process.env.WARMUP ?? 3);

const url = new URL(TARGET);
const baseURL = url.origin;
const path = url.pathname;

const signer = privateKeyToAccount(KEY.startsWith("0x") ? KEY : `0x${KEY}`);
const client = new x402Client();
client.register("eip155:*", new ExactEvmScheme(signer));
const api = wrapAxiosWithPayment(axios.create({ baseURL }), client);

const body = {
  model: "router-default",
  messages: [{ role: "user", content: "Reply with the single word: ok" }],
  max_tokens: 16,
};

function pct(sorted, p) {
  if (sorted.length === 0) return 0;
  const i = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[i];
}

function summarize(name, values) {
  const s = [...values].sort((a, b) => a - b);
  console.log(
    `${name.padEnd(12)} median=${pct(s, 50)} p95=${pct(s, 95)} p99=${pct(s, 99)} (us, n=${s.length})`,
  );
}

async function once() {
  const r = await api.post(path, body);
  const h = r.headers;
  return {
    verify: Number(h["x-verify-us"] ?? 0),
    upstream: Number(h["x-upstream-us"] ?? 0),
    settle: Number(h["x-settle-us"] ?? 0),
    overhead: Number(h["x-overhead-us"] ?? 0),
    total: Number(h["x-total-us"] ?? 0),
  };
}

const cols = { verify: [], upstream: [], settle: [], overhead: [], total: [] };
let failures = 0;
let lastError = "";

for (let i = 0; i < WARMUP; i++) {
  try {
    await once();
  } catch {
    /* warmup errors ignored */
  }
}
for (let i = 0; i < RUNS; i++) {
  try {
    const m = await once();
    for (const k of Object.keys(cols)) cols[k].push(m[k]);
    process.stdout.write(".");
  } catch (e) {
    failures += 1;
    lastError = e?.response?.status
      ? `HTTP ${e.response.status}`
      : (e?.code ?? e?.message ?? "unknown");
    process.stdout.write("x");
  }
}
process.stdout.write("\n");

for (const k of Object.keys(cols)) summarize(k, cols[k]);
console.log(
  `samples=${cols.total.length} failures=${failures}` +
    (failures ? ` lastError=${lastError}` : ""),
);
