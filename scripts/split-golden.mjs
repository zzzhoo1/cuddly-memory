#!/usr/bin/env node
/**
 * Derive the per-protocol golden split files from golden.json.
 *
 * golden.json (produced by gen-golden.mts, the single source of truth) carries
 * all protocols in one `responses` array. Each crate embeds only its own half:
 *
 *   golden_decisions_openai.json + golden_responses_openai.json  (root)
 *   rust-port/golden_decisions_anthropic.json
 *   rust-port/golden_responses_anthropic.json
 *
 * Usage:
 *   node scripts/split-golden.mjs           # write the split files
 *   node scripts/split-golden.mjs --check   # fail (exit 1) if any is stale
 */

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE = join(ROOT, "golden.json");

/** [protocol, output path, kind] */
const OUTPUTS = [
  ["openai", join(ROOT, "golden_decisions_openai.json"), "decisions"],
  ["openai", join(ROOT, "golden_responses_openai.json"), "responses"],
  ["anthropic", join(ROOT, "rust-port", "golden_decisions_anthropic.json"), "decisions"],
  ["anthropic", join(ROOT, "rust-port", "golden_responses_anthropic.json"), "responses"],
];

const check = process.argv.includes("--check");
const golden = JSON.parse(readFileSync(SOURCE, "utf8"));

let stale = 0;
for (const [protocol, path, kind] of OUTPUTS) {
  const rows = golden.responses.filter((r) => r.protocol === protocol);
  const payload =
    kind === "decisions"
      ? rows.map((r) => r.decision)
      : rows.map((r) => ({ status: r.status, headers: r.headers, body: r.body }));
  const text = JSON.stringify(payload);

  if (check) {
    if (readFileSync(path, "utf8") !== text) {
      console.error(`stale: ${path}`);
      stale += 1;
    }
  } else {
    writeFileSync(path, text);
    console.log(`wrote ${path} (${payload.length} rows)`);
  }
}

if (check) {
  if (stale > 0) {
    console.error(`${stale} split file(s) out of sync with golden.json — run: node scripts/split-golden.mjs`);
    process.exit(1);
  }
  console.log("split files in sync with golden.json");
}
