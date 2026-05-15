// x402 V2 buyer using the batch-settlement scheme. First request opens
// a channel via a gasless (facilitator-sponsored) on-chain deposit;
// every subsequent request is a pure off-chain cumulative voucher.
// Resilient: failures are counted, the run continues, the summary
// reports successful samples plus a failure count.
//
// Env: EVM_PRIVATE_KEY (Base Sepolia, funded), TARGET_URL, RUNS, WARMUP,
//      RPC_URL (default https://sepolia.base.org)

import axios from "axios";
import { x402Client, wrapAxiosWithPayment } from "@x402/axios";
import { BatchSettlementEvmScheme } from "@x402/evm/batch-settlement/client";
import { privateKeyToAccount } from "viem/accounts";

const KEY = process.env.EVM_PRIVATE_KEY;
if (!KEY) {
  console.error("EVM_PRIVATE_KEY is required");
  process.exit(1);
}
const TARGET = process.env.TARGET_URL ?? "http://127.0.0.1:8080/v1/chat/completions";
const RUNS = Number(process.env.RUNS ?? 20);
const WARMUP = Number(process.env.WARMUP ?? 2);
const RPC_URL = process.env.RPC_URL ?? "https://sepolia.base.org";

const url = new URL(TARGET);
const baseURL = url.origin;
const path = url.pathname;

const signer = privateKeyToAccount(KEY.startsWith("0x") ? KEY : `0x${KEY}`);
const client = new x402Client();
// Large deposit multiplier: one deposit funds the whole run so the
// measured set is pure vouchers (no mid-run channel top-ups).
client.register(
  "eip155:*",
  new BatchSettlementEvmScheme(signer, {
    rpcUrl: RPC_URL,
    depositPolicy: { depositMultiplier: 100 },
  }),
);
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
    kind: h["x-batch-kind"] ?? "?",
  };
}

const cols = { verify: [], upstream: [], settle: [], overhead: [], total: [] };
let failures = 0;
let lastError = "";

for (let i = 0; i < WARMUP; i++) {
  try {
    const m = await once();
    process.stdout.write(`[warmup ${m.kind}]`);
  } catch (e) {
    lastError = e?.response?.status ? `HTTP ${e.response.status}` : (e?.code ?? e?.message);
    process.stdout.write("[warmup x]");
  }
}
process.stdout.write("\n");
for (let i = 0; i < RUNS; i++) {
  try {
    const m = await once();
    for (const k of Object.keys(cols)) cols[k].push(m[k]);
    process.stdout.write(m.kind === "voucher" ? "." : "D");
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
