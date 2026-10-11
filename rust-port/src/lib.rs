//! Fidelity port of the **anthropic** half of the rate-limit 429 response builder.
//!
//! Reference implementation (TypeScript, do not edit):
//!   `/var/minis/shared/tdba/repo/MemoryProxy/src/rate-limit/guard.ts`
//!   -> `buildRateLimitResponse(protocol, decision)`  (guard.ts:96)
//!
//! Golden data: the 5 entries of `golden.json` with `protocol == "anthropic"`,
//! filtered out of the 10-entry `responses` array and re-serialized as compact
//! JSON arrays. Embedded via `include_str!` so the literals never live in code.
//!
//! ---------------------------------------------------------------------------
//! Verified clamping rules (checked against guard.ts + golden.json)
//! ---------------------------------------------------------------------------
//! 1. `Number()` -> `f64` -> `floor()` -> clamp at 0 is the **store/usage** rule
//!    (`redis-store.ts:192` does `Number(raw[8])`; `usage.ts` `numberField()`
//!    does `Math.max(0, Math.floor(v))`). `buildRateLimitResponse` itself does NO
//!    floor and NO clamp at 0 -- see rule 2.
//!
//! 2. The two retry headers are built with `String(Math.max(1, decision.retryAfterSeconds))`.
//!    So the floor is **1, not 0**:
//!      - case9  retryAfterSeconds = -3   -> `retry-after: "1"`, `x-ratelimit-reset-seconds: "1"`
//!      - case2  retryAfterSeconds = 0    -> `retry-after: "1"`, `x-ratelimit-reset-seconds: "1"`
//!      - case3  retryAfterSeconds = 86400-> emitted verbatim (no upper clamp)
//!    Note `Math.max` is applied to the *decision field*, not to a floored value,
//!    so a fractional input (e.g. 1.9) would stay 1.9 -- `String(1.9)` = "1.9".
//!    No golden case covers that; the port keeps JS `Number -> String` semantics.
//!
//! 3. `degraded` contributes **no header and no body field at all**. The builder
//!    never reads `decision.degraded`; the degraded branch lives in
//!    `enforceRateLimit` (guard.ts:44-52) and is evaluated *before* the builder
//!    runs. In golden.json the anthropic case 5 `degraded` renders as `{}`
//!    because `gen-golden.mts` passed `new Error(...)` and its `canon()`
//!    walks own enumerable keys only (an Error has none).
//!    So: "degraded is false or {} -> the header is absent" is NOT the rule --
//!    that header never exists for any degraded value. The port models
//!    `degraded` as an opaque `serde_json::Value` to stay byte-compatible.
//!
//! 4. The Chinese error copy is embedded as UTF-8 bytes and compared byte-for-byte.
//!    reason == "qpm" -> "该模型请求频率已达上限，请稍后重试"
//!    reason != "qpm" -> "该模型输入 Token 用量已达上限，请稍后重试"
//!    (guard.ts:98-100 -- note the `!==` means ANY non-"qpm" reason, including
//!    an unexpected one, gets the tpm copy.)
//!
//! 5. golden.json `body` is `JSON.parse(await res.text())` run through `canon()`,
//!    which sorts object keys. The **on-the-wire** byte order from the real TS is
//!    `{"type":"error","error":{"type":"...","message":"..."}}` (insertion order,
//!    guard.ts:101-105), whereas golden.json shows `{"error":{...},"type":"error"}`.
//!    Golden therefore cannot express wire bytes; the tests assert golden equality
//!    structurally (via `serde_json::Value`) and separately assert the true wire
//!    form reconstructed from guard.ts. See `expected_wire_body`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Compact JSON of the 5 anthropic `decision` objects (golden.json, `body` removed).
pub const GOLDEN_DECISIONS_ANTHROPIC: &str = include_str!("../golden_decisions_anthropic.json");

/// Compact JSON of the 5 anthropic `{status, headers, body}` triples.
pub const GOLDEN_RESPONSES_ANTHROPIC: &str = include_str!("../golden_responses_anthropic.json");

/// A decision as produced by `RedisRateLimitStore.checkRequest`.
///
/// `degraded` is intentionally an opaque `Value`: in golden.json it is `false`
/// for cases 1-4 and `{}` for case 5 (a canonicalized `new Error()`), and the
/// builder never inspects it (rule 3 above).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub allowed: bool,
    pub degraded: Value,
    pub reason: String,
    pub tpm: Value,
    pub qpm: Value,
    pub used_tokens: Value,
    pub used_requests: Value,
    pub remaining_tokens: Value,
    pub remaining_requests: Value,
    pub retry_after_seconds: Value,
}

/// `Number()` coercion as applied to numeric decision fields.
///
/// The store already does `Number(raw[i])` before handing the decision to the
/// builder (`redis-store.ts:190-192`), so every numeric field reaching
/// `buildRateLimitResponse` is a real JS number. The golden cases are all plain
/// JSON numbers, so this is the identity path; anything else falls back to 0.0,
/// matching `usage.ts` `numberField()` for unusable input.
pub fn to_f64(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// One golden response triple.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoldenResponse {
    pub status: u16,
    /// Header name -> value; compared key by key.
    pub headers: std::collections::BTreeMap<String, String>,
    /// Golden body is a key-sorted `canon()` rendering of `JSON.parse(res.text())`,
    /// so it is compared as a `Value`, not as raw bytes (see rule 5).
    pub body: Value,
}

/// The response the TS builder actually returns.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltResponse {
    pub status: u16,
    /// Header name (lowercase, as `Response` normalizes) -> value.
    pub headers: std::collections::BTreeMap<String, String>,
    /// Exact bytes the TS `Response` body would carry.
    pub body_bytes: Vec<u8>,
}

/// The rebuilt 429, mirroring guard.ts `buildRateLimitResponse`.
pub fn build_rate_limit_response(decision: &Decision) -> BuiltResponse {
    // guard.ts:98-100 -- strict `=== "qpm"`, so every other reason gets the tpm copy.
    let message: &str = if decision.reason == "qpm" {
        "该模型请求频率已达上限，请稍后重试"
    } else {
        "该模型输入 Token 用量已达上限，请稍后重试"
    };

    // guard.ts:101-105 -- anthropic branch only.
    let body = serde_json::json!({
        "type": "error",
        "error": { "type": "rate_limit_error", "message": message },
    });
    let body_bytes = serde_json::to_vec(&body).expect("body is infallibly serializable");

    let retry = to_f64(&decision.retry_after_seconds);

    let mut headers = std::collections::BTreeMap::new();
    headers.insert(
        "content-type".to_string(),
        "application/json; charset=utf-8".to_string(),
    );
    // Math.max(1, retryAfterSeconds) -> String(...). Both retry headers share it.
    let retry_header = js_number_to_string(f64::max(1.0, retry));
    headers.insert("retry-after".to_string(), retry_header.clone());
    headers.insert("x-ratelimit-reset-seconds".to_string(), retry_header);
    headers.insert(
        "x-ratelimit-limit-input-tokens".to_string(),
        js_number_to_string(to_f64(&decision.tpm)),
    );
    headers.insert(
        "x-ratelimit-remaining-input-tokens".to_string(),
        js_number_to_string(to_f64(&decision.remaining_tokens)),
    );
    headers.insert(
        "x-ratelimit-limit-requests".to_string(),
        js_number_to_string(to_f64(&decision.qpm)),
    );
    headers.insert(
        "x-ratelimit-remaining-requests".to_string(),
        js_number_to_string(to_f64(&decision.remaining_requests)),
    );

    BuiltResponse {
        status: 429,
        headers,
        body_bytes,
    }
}

/// Reproduce JS `String(n)` for the header values.
///
/// The golden corpus only contains integers, so `f64` -> i64 is lossless here.
/// JS renders integral values without a `.0` suffix and keeps the shortest
/// round-tripping decimal otherwise; Rust's `{}` for f64 matches that for
/// ordinary magnitudes, but the two differ for very large values (JS switches to
/// exponential at 1e21). Out of scope for these 5 cases; kept simple and noted.
fn js_number_to_string(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

/// The exact bytes the real TS `Response` carries, reconstructed from guard.ts
/// insertion order (golden.json cannot express this -- see rule 5).
pub fn expected_wire_body(decision: &Decision) -> Vec<u8> {
    let message: &str = if decision.reason == "qpm" {
        "该模型请求频率已达上限，请稍后重试"
    } else {
        "该模型输入 Token 用量已达上限，请稍后重试"
    };
    // Hand-written in guard.ts literal order, NOT via serde_json::json! macro,
    // so the byte order is fixed by this function and not by a map.
    format!(
        r#"{{"type":"error","error":{{"type":"rate_limit_error","message":"{}"}}}}"#,
        message
    )
    .into_bytes()
}
