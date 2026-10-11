/**
 * Golden generator for the rate-limit PoC diff test.
 *
 * Runs the REAL TS implementation via tsx and dumps canonical JSON,
 * so the Rust side is verified against actual behavior, not a re-derived
 * spec that could share the translator's mistakes.
 *
 * Usage:
 *   MEMORYPROXY_REPO=/path/to/MemoryProxy npx tsx gen-golden.mts > golden.json
 *
 * MEMORYPROXY_REPO defaults to the development checkout below. CI points it at
 * a fresh checkout of the pinned upstream commit before regenerating, so the
 * committed golden.json is provably reproducible from the reference
 * implementation (see .github/workflows/ci.yml, job "golden-repro").
 */

const REPO = process.env.MEMORYPROXY_REPO ?? "/var/minis/shared/tdba/repo/MemoryProxy";
import { createHash } from "node:crypto";
const { getActualInputTokens } = await import(`${REPO}/src/rate-limit/usage.js`);
const { buildRateLimitResponse } = await import(`${REPO}/src/rate-limit/guard.js`);
const { dimensionField, RedisRateLimitStore } = await import(`${REPO}/src/rate-limit/redis-store.js`);

// ---------- canonicalization ----------
// NaN/Infinity are not valid JSON; encode them as strings so a NaN result
// can never silently equal an Infinity result via JSON's null fallback.
function canon(v) {
  if (typeof v === "number") {
    if (Number.isNaN(v)) return "NaN";
    if (v === Infinity) return "Infinity";
    if (v === -Infinity) return "-Infinity";
    return v;
  }
  if (v === null || typeof v !== "object") return v === undefined ? null : v;
  if (Array.isArray(v)) return v.map(canon);
  const out = {};
  for (const k of Object.keys(v).sort()) {
    if (v[k] === undefined) continue; // mimic JSON.stringify
    out[k] = canon(v[k]);
  }
  return out;
}

// ---------- 1. usage normalization ----------
const usageCases = [
  { protocol: "openai", usage: null },
  { protocol: "anthropic", usage: null },
  { protocol: "openai", usage: undefined },
  { protocol: "openai", usage: {} },
  { protocol: "anthropic", usage: {} },
  { protocol: "openai", usage: { prompt_tokens: 100 } },
  { protocol: "openai", usage: { prompt_tokens: 100.7 } },
  { protocol: "openai", usage: { prompt_tokens: -5 } },
  { protocol: "openai", usage: { prompt_tokens: 0 } },
  { protocol: "openai", usage: { prompt_tokens: "42" } },
  { protocol: "openai", usage: { prompt_tokens: "42.9" } },
  { protocol: "openai", usage: { prompt_tokens: "-3" } },
  { protocol: "openai", usage: { prompt_tokens: "abc" } },
  { protocol: "openai", usage: { prompt_tokens: "" } },
  { protocol: "openai", usage: { prompt_tokens: NaN } },
  { protocol: "openai", usage: { prompt_tokens: Infinity } },
  { protocol: "openai", usage: { prompt_tokens: 1e21 } },
  { protocol: "anthropic", usage: { input_tokens: 100, cache_read_input_tokens: 50, cache_creation_input_tokens: 25 } },
  { protocol: "anthropic", usage: { input_tokens: 100 } },
  { protocol: "anthropic", usage: { input_tokens: 0, cache_read_input_tokens: 0, cache_creation_input_tokens: 0 } },
  { protocol: "anthropic", usage: { input_tokens: "10", cache_read_input_tokens: null, cache_creation_input_tokens: {} } },
  { protocol: "anthropic", usage: { input_tokens: 70.2, cache_read_input_tokens: -1, cache_creation_input_tokens: "8" } },
  { protocol: "openai", usage: { input_tokens: 70, cache_read_input_tokens: 10 } }, // openai ignores anthropic fields
  { protocol: "anthropic", usage: { prompt_tokens: 500 } }, // anthropic ignores openai fields
  // Boundary cases — same provenance rule as everything above: every value in
  // golden.json, including these, must come out of this script. Never hand-edit
  // golden.json; a value that only exists there is unreproducible.
  { protocol: "openai", usage: { prompt_tokens: " 7" }, note: "leading whitespace" },
  { protocol: "openai", usage: { prompt_tokens: "0x10" }, note: "hex parse" },
  { protocol: "openai", usage: { prompt_tokens: "\u2028" }, note: "line separator -> NaN -> 0" },
  { protocol: "openai", usage: { prompt_tokens: "1e21" }, note: "exceeds i64, stays float" },
];
const usage = usageCases.map((c) => ({
  ...c,
  expected: getActualInputTokens(c.usage, c.protocol),
}));

// ---------- 2. 429 response construction ----------
const decisions = [
  { allowed: false, degraded: false, reason: "qpm", tpm: 500, qpm: 10, usedTokens: 123, usedRequests: 11, remainingTokens: 377, remainingRequests: 0, retryAfterSeconds: 37 },
  { allowed: false, degraded: false, reason: "tpm", tpm: 1000, qpm: 20, usedTokens: 1000, usedRequests: 3, remainingTokens: 0, remainingRequests: 17, retryAfterSeconds: 0 },
  { allowed: false, degraded: false, reason: "tpm", tpm: 1000, qpm: 20, usedTokens: 1200, usedRequests: 3, remainingTokens: 0, remainingRequests: 17, retryAfterSeconds: 86400 },
  { allowed: false, degraded: false, reason: "qpm", tpm: 0, qpm: 5, usedTokens: 0, usedRequests: 6, remainingTokens: 0, remainingRequests: 0, retryAfterSeconds: 1 },
  { allowed: false, degraded: new Error("degraded is ignored by builder"), reason: "qpm", tpm: 500, qpm: 5, usedTokens: 9, usedRequests: 6, remainingTokens: 0, remainingRequests: 0, retryAfterSeconds: -3 },
];
const responses = [];
for (const decision of decisions) {
  for (const protocol of ["openai", "anthropic"]) {
    const res = buildRateLimitResponse(protocol, decision);
    responses.push({
      protocol,
      decision,
      status: res.status,
      headers: Object.fromEntries(res.headers.entries()),
      body: JSON.parse(await res.text()),
    });
  }
}

// ---------- 3. dimensionField ----------
const dimensionCases = [
  ["inst-1", "gpt-4o"],
  ["", ""],
  ['with"quote', "back\\slash"],
  ["unicode-é中文", "模型-🚀"],
  ["line\nbreak", "tab\there"],
  ["x".repeat(300), "y".repeat(300)],
].map(([instanceId, modelId]) => ({ instanceId, modelId, expected: dimensionField(instanceId, modelId) }));

// ---------- 4. RedisRateLimitStore (deterministic surface) ----------
// The store's network paths (Lua eval) have no oracle without a live Redis.
// What IS deterministic and worth pinning: the key derivation (dimension →
// sha256 → 32-hex namespace), the degraded decision when no client exists, the
// `getLimits` fallback, and the argv/keys the CHECK and RECORD calls would send
// (captured through a stub client that records instead of talking to Redis).
const digest32 = (s) => createHash("sha256").update(s).digest("hex").slice(0, 32);

const storeKeyCases = [
  ["inst-1", "gpt-4o"],
  ["", ""],
  ['with"quote', "back\\slash"],
  ["unicode-é中文", "模型-🚀"],
  ["x".repeat(300), "y".repeat(300)],
].map(([instanceId, modelId]) => {
  const dimension = dimensionField(instanceId, modelId);
  const digest = digest32(dimension);
  return {
    instanceId,
    modelId,
    dimension,
    digest,
    reqIndex: `{rl}:req:${digest}:idx`,
    reqCounts: `{rl}:req:${digest}:cnt`,
    tokIndex: `{rl}:tok:${digest}:idx`,
    tokCounts: `{rl}:tok:${digest}:cnt`,
  };
});

const STORE_CONFIG = { tpm: 1000, qpm: 20 };

/** Stub ioredis client: records the call instead of performing it. */
function makeStubClient() {
  const calls = [];
  return {
    calls,
    async eval(script, numKeys, ...rest) {
      // Record what the store actually sends, but keep golden compact: the Lua
      // body is summarized by its digest (that still pins WHICH script ran).
      calls.push({
        op: "eval",
        scriptDigest: createHash("sha256").update(script).digest("hex").slice(0, 32),
        numKeys,
        keys: rest.slice(0, numKeys),
        argv: rest.slice(numKeys),
      });
      // Shape of the real Lua reply: 9 numbers.
      return [1, 0, 1000, 20, 500, 3, 500, 17, 0];
    },
    async hmget(key, ...fields) {
      calls.push({ op: "hmget", key, fields });
      return [null, null];
    },
    async hset(key, values) {
      calls.push({ op: "hset", key, values });
      return 1;
    },
    async hget(key, field) {
      calls.push({ op: "hget", key, field });
      return null;
    },
    async hgetall(key) {
      calls.push({ op: "hgetall", key });
      return {};
    },
    async hdel(key, ...fields) {
      calls.push({ op: "hdel", key, fields });
      return 1;
    },
  };
}

const storeCalls = [];
{
  const stub = makeStubClient();
  const store = new RedisRateLimitStore(stub, STORE_CONFIG);
  await store.checkRequest("inst-1", "gpt-4o");
  await store.recordInputTokens("inst-1", "gpt-4o", 42.7);
  await store.recordInputTokens("inst-1", "gpt-4o", 0); // early return, no eval
  storeCalls.push({ op: "checkRequest+recordInputTokens", calls: stub.calls });

  const degradedStore = new RedisRateLimitStore(null, STORE_CONFIG);
  storeCalls.push({
    op: "degradedDecision(no client)",
    isAvailable: degradedStore.isAvailable(),
    check: await degradedStore.checkRequest("inst-1", "gpt-4o"),
    record: await degradedStore.recordInputTokens("inst-1", "gpt-4o", 10),
  });
}

// ---------- output ----------
process.stdout.write(JSON.stringify(
  canon({ usage, responses, dimension: dimensionCases, storeKeys: storeKeyCases, storeOps: storeCalls }),
  null,
  2,
));
