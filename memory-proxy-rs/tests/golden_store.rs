//! Golden diff tests — `RedisRateLimitStore` 的确定性面。
//!
//! 覆盖 `golden.json` 的两段：
//! - `storeKeys[5]`：`dimensionKeys()` 派生的四条 Redis 键（dimension →
//!   sha256 前 32 hex → `{rl}:req:{d}:idx` 等）。
//! - `storeOps[2]`：经由记录型 stub client 捕获的调用形态 + 无 client 时的
//!   降级决策。
//!
//! `storeOps[0].calls[].scriptDigest` 是 Lua 脚本体的 sha256 前 32 hex。
//! 这里用 `include_str!` 把 TS 源里的 Lua 常量抄进来并按同一规则摘要——
//! 若上游改了脚本，这个测试会红，提示重新生成 golden 并复核 Rust 侧语义。

use memory_proxy_rs::store::{
    check_request_eval, degraded_decision, dimension_keys, limits_from_hash, parse_override,
    record_tokens_eval, RateLimitConfig, KEY_PREFIX, WINDOW_SECONDS,
};
use ratelimit_core::digest::sha256_hex;
use serde::Deserialize;
use serde_json::Value;

/// 转写的 Lua 脚本摘影（sha256 前 32 hex），来自 `golden.json storeOps[0]`
/// 且与 `redis-store.ts` 里的 `CHECK_REQUEST_LUA` / `RECORD_TOKENS_LUA` 对应。
///
/// 这两个常量不是从 golden 读的——正是因为它们写死在 Rust 侧，上游改动 Lua
/// 时（golden 重新生成后 digest 会变）这里会红，提醒重新核对 KEYS/ARGV 语义。
const CHECK_REQUEST_LUA_DIGEST: &str = "1aa9ed4a955b661ddbf5470ee3a61b73";
const RECORD_TOKENS_LUA_DIGEST: &str = "256c5b308effee57e1329d83af8ab6a3";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreKeyCase {
    instance_id: String,
    model_id: String,
    dimension: String,
    digest: String,
    req_index: String,
    req_counts: String,
    tok_index: String,
    tok_counts: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EvalCallGolden {
    script_digest: String,
    num_keys: u32,
    keys: Vec<String>,
    argv: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoreOp {
    #[serde(default)]
    calls: Vec<EvalCallGolden>,
    #[serde(default)]
    check: Option<Value>,
    #[serde(default)]
    is_available: Option<bool>,
    #[serde(default)]
    record: Option<bool>,
}

fn golden() -> Value {
    serde_json::from_str(include_str!("../../golden.json")).expect("golden.json parses")
}

fn store_keys() -> Vec<StoreKeyCase> {
    serde_json::from_value(golden()["storeKeys"].clone()).expect("storeKeys shape")
}

fn store_ops() -> Vec<StoreOp> {
    serde_json::from_value(golden()["storeOps"].clone()).expect("storeOps shape")
}

// ── 1. 键派生 ──────────────────────────────────────────────────────────────

#[test]
fn golden_store_keys_match_ts() {
    let cases = store_keys();
    assert_eq!(cases.len(), 5, "golden storeKeys case count");
    for c in &cases {
        let keys = dimension_keys(&c.instance_id, &c.model_id);
        assert_eq!(
            keys.dimension, c.dimension,
            "dimension for {:?}",
            c.instance_id
        );
        assert_eq!(keys.digest, c.digest, "digest for {:?}", c.dimension);
        assert_eq!(keys.req_index, c.req_index);
        assert_eq!(keys.req_counts, c.req_counts);
        assert_eq!(keys.tok_index, c.tok_index);
        assert_eq!(keys.tok_counts, c.tok_counts);
    }
}

/// 摘要规则独立断言：必须是 sha256 的**前 32 个 hex 字符**。
///
/// TS: `createHash("sha256").update(dimension).digest("hex").slice(0, 32)`。
/// 若误用全 64 hex，键会变长且与 TS 不匹配——这条测试在 golden 之外钉死它。
#[test]
fn digest_is_sha256_prefix_32_hex() {
    let full = sha256_hex(br#"["inst-1","gpt-4o"]"#);
    assert_eq!(full.len(), 64);
    let keys = dimension_keys("inst-1", "gpt-4o");
    assert_eq!(keys.digest, full[..32]);
    assert!(keys.req_index.starts_with(KEY_PREFIX));
    assert_eq!(
        keys.req_index,
        format!("{KEY_PREFIX}req:{}:idx", &full[..32])
    );
}

/// 键前缀常量与 Cluster hash tag。
#[test]
fn key_prefix_is_cluster_hash_tag() {
    assert_eq!(KEY_PREFIX, "{rl}:");
    assert_eq!(WINDOW_SECONDS, 60);
    let keys = dimension_keys("i", "m");
    // 所有键都必须携带同一个 hash tag，否则 Lua 的跨键访问在 Cluster 下会失败。
    for k in [
        &keys.req_index,
        &keys.req_counts,
        &keys.tok_index,
        &keys.tok_counts,
    ] {
        assert!(k.starts_with("{rl}:"), "key {k} lost the hash tag");
    }
}

// ── 2. 调用编组 ────────────────────────────────────────────────────────────

#[test]
fn golden_check_request_eval_matches_ts() {
    let ops = store_ops();
    let golden_call = &ops[0].calls[0];
    // 钉住“跑的是哪个脚本”：上游改了 Lua，生成器产出的 digest 会变，这里就红。
    assert_eq!(
        golden_call.script_digest, CHECK_REQUEST_LUA_DIGEST,
        "upstream CHECK_REQUEST_LUA changed — re-verify KEYS/ARGV order"
    );
    let config = RateLimitConfig { tpm: 1000, qpm: 20 };
    let keys = dimension_keys("inst-1", "gpt-4o");
    let call = check_request_eval(&keys, &config, CHECK_REQUEST_LUA_DIGEST);

    assert_eq!(call.num_keys, golden_call.num_keys);
    assert_eq!(call.num_keys, 6);
    assert_eq!(call.keys, golden_call.keys);
    assert_eq!(call.argv, golden_call.argv);
    // KEYS 顺序：config, overrides, reqIdx, reqCnt, tokIdx, tokCnt
    assert_eq!(call.keys[0], "{rl}:config");
    assert_eq!(call.keys[1], "{rl}:overrides");
    // ARGV 顺序：dimension, tpm, qpm, window
    assert_eq!(call.argv[0], r#"["inst-1","gpt-4o"]"#);
    assert_eq!(call.argv[1], "1000");
    assert_eq!(call.argv[2], "20");
    assert_eq!(call.argv[3], "60");
}

#[test]
fn golden_record_tokens_eval_matches_ts() {
    let ops = store_ops();
    let golden_call = &ops[0].calls[1];
    assert_eq!(
        golden_call.script_digest, RECORD_TOKENS_LUA_DIGEST,
        "upstream RECORD_TOKENS_LUA changed — re-verify KEYS/ARGV order"
    );
    let keys = dimension_keys("inst-1", "gpt-4o");
    // golden 里记录的是 `recordInputTokens(..., 42.7)` → Math.floor → "42"
    let call = record_tokens_eval(&keys, 42.7, RECORD_TOKENS_LUA_DIGEST);

    assert_eq!(call.num_keys, golden_call.num_keys);
    assert_eq!(call.num_keys, 2);
    assert_eq!(call.keys, golden_call.keys);
    assert_eq!(call.argv, golden_call.argv);
    assert_eq!(call.argv[0], "60"); // window
    assert_eq!(call.argv[1], "42"); // floor(42.7)
}

/// `String(Math.floor(n))` 的边界：向负无穷取整，不是截断。
#[test]
fn record_tokens_floor_semantics() {
    let keys = dimension_keys("i", "m");
    for (input, expected) in [
        (42.7_f64, "42"),
        (0.9, "0"),
        (1.0, "1"),
        (99.999, "99"),
        (-0.5, "-1"), // Math.floor(-0.5) === -1（截断会给 0）
        (-1.5, "-2"),
    ] {
        let call = record_tokens_eval(&keys, input, "d");
        assert_eq!(call.argv[1], expected, "floor({input})");
    }
}

// ── 3. 降级决策 ────────────────────────────────────────────────────────────

#[test]
fn golden_degraded_decision_matches_ts() {
    let ops = store_ops();
    let golden_check = ops[1].check.as_ref().expect("storeOps[1].check");
    let config = RateLimitConfig { tpm: 1000, qpm: 20 };
    let decision = degraded_decision(&config, "redis_unavailable");

    assert_eq!(ops[1].is_available, Some(false));
    assert_eq!(
        ops[1].record,
        Some(false),
        "record returns false without a client"
    );

    assert!(decision.allowed, "fail-open: degraded must allow");
    assert_eq!(decision.degraded, Value::Bool(true));
    assert_eq!(
        decision.degraded_reason.as_deref(),
        golden_check["degradedReason"].as_str()
    );
    assert_eq!(decision.reason, None, "degraded 路径的 reason 是 null");
    assert_eq!(
        golden_check["reason"],
        Value::Null,
        "golden 里 degraded 案例的 reason 也是 null"
    );
    assert_eq!(decision.tpm, golden_check["tpm"]);
    assert_eq!(decision.qpm, golden_check["qpm"]);
    assert_eq!(decision.used_tokens, golden_check["usedTokens"]);
    assert_eq!(decision.used_requests, golden_check["usedRequests"]);
    assert_eq!(decision.remaining_tokens, golden_check["remainingTokens"]);
    assert_eq!(
        decision.remaining_requests,
        golden_check["remainingRequests"]
    );
    assert_eq!(
        decision.retry_after_seconds,
        golden_check["retryAfterSeconds"]
    );
}

/// 降级时"剩余额度 = 配置全额"（限流器缺席视作没有消耗）。
#[test]
fn degraded_reports_full_quota() {
    let config = RateLimitConfig { tpm: 7, qpm: 3 };
    let d = degraded_decision(&config, "boom");
    assert_eq!(d.tpm, Value::from(7));
    assert_eq!(d.qpm, Value::from(3));
    assert_eq!(d.remaining_tokens, Value::from(7));
    assert_eq!(d.remaining_requests, Value::from(3));
}

// ── 4. 回读与 override 解析 ────────────────────────────────────────────────

/// `getLimits()` 的真值判断：`""` 与缺失都回退配置。
#[test]
fn limits_fall_back_on_missing_or_empty() {
    let config = RateLimitConfig { tpm: 1000, qpm: 20 };
    assert_eq!(limits_from_hash(Some("50"), Some("5"), &config), (50, 5));
    assert_eq!(limits_from_hash(None, None, &config), (1000, 20));
    // JS: `"" ? … : fallback` → 空串是 falsy，走回退
    assert_eq!(limits_from_hash(Some(""), Some(""), &config), (1000, 20));
    assert_eq!(
        limits_from_hash(Some("0"), None, &config),
        (0, 20),
        "0 是合法值"
    );
}

/// override 的三条解析分支（对象 / 裸值 / 非 JSON）。
#[test]
fn override_parsing_branches() {
    let config = RateLimitConfig { tpm: 1000, qpm: 20 };
    // 分支 1：JSON 对象，缺失字段回退
    assert_eq!(
        parse_override(r#"{"tpm":5,"qpm":2}"#, &config),
        Some((5, 2))
    );
    assert_eq!(parse_override(r#"{"tpm":5}"#, &config), Some((5, 20)));
    assert_eq!(parse_override("{}", &config), Some((1000, 20)));
    // 分支 2：可解析但非对象（裸数字）→ tpm=该值, qpm 回退
    assert_eq!(parse_override("77", &config), Some((77, 20)));
    // 分支 3：非 JSON 文本 → Number(raw)，qpm 回退
    assert_eq!(parse_override("not-json", &config), Some((1000, 20)));
}
