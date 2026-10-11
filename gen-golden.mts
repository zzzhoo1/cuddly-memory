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
const { getActualInputTokens } = await import(`${REPO}/src/rate-limit/usage.js`);
const { buildRateLimitResponse } = await import(`${REPO}/src/rate-limit/guard.js`);
const { dimensionField } = await import(`${REPO}/src/rate-limit/redis-store.js`);

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

// ---------- output ----------
process.stdout.write(JSON.stringify(
  canon({ usage, responses, dimension: dimensionCases }),
  null,
  2,
));
