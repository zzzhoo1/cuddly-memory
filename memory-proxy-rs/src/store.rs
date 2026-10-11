//! `redis-store` 确定性面的保真移植：键派生、降级决策、以及 CHECK/RECORD
//! 两条 eval 的 KEYS/ARGV 编组。
//!
//! 参考实现（TypeScript，禁止改动）：
//!   `/var/minis/shared/tdba/repo/MemoryProxy/src/rate-limit/redis-store.ts`
//!
//! golden 段：`golden.json` 的 `storeKeys`（5 例）与 `storeOps`（2 例），
//! 由 `gen-golden.mts` 直接执行真实 TS 产出。`storeOps[0].calls` 是经由
//! **记录型 stub client** 捕获的调用——Lua 脚本本体以 `scriptDigest`
//! （sha256 前 32 hex）参与比对：这既钉住"跑的是哪个脚本"，又不必把 Lua
//! 源码抄进 golden。
//!
//! # 显式不在范围
//!
//! - 真实网络 IO（ioredis 连接、重试、`eval` 往返）：无 oracle 可 diff。
//! - `listOverrides()` 的排序：TS 用 `String.prototype.localeCompare`，
//!   其结果依赖 Node 的 ICU 与默认 locale（本机 en-US），与码点序在
//!   `"a1"`/`"A1"`、`"_x"`/`"0a"`、`"a"`/`"B"` 等处**分歧**（已实测）。
//!   该函数因此不是环境无关的确定性面——照搬 locale 语义不可能字节保真，
//!   本 crate 只移植其**解析**部分（字段清洗），排序留给调用方/未来实现。
//!
//! # 与 A 类移植的分工
//!
//! `decision.rs` / `guard.rs` 覆盖 guard 层；本模块覆盖 store 层。两者共用
//! [`crate::decision::Decision`]（store 的 `checkRequest` 产出该结构）。

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::decision::Decision;

/// `redis-store.ts:6` — `const WINDOW_SECONDS = 60;`
pub const WINDOW_SECONDS: u64 = 60;

/// `redis-store.ts:13` — `const KEY_PREFIX = "{rl}:";`
///
/// `{rl}` 是常量 Cluster hash tag：所有键落同一 slot，CHECK 的 Lua 才能一次
/// EVAL 里同时触碰 config / overrides / 某个维度的计数器。
pub const KEY_PREFIX: &str = "{rl}:";

/// 一个 `instance × model` 维度的 Redis 键组。
///
/// 源：`redis-store.ts:dimensionKeys()`。`dimension` 是
/// [`dimension_field`](crate::dimension::dimension_field) 的结果，
/// digest 取 sha256 的**前 32 个 hex 字符**（`slice(0, 32)`）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimensionKeys {
    pub instance_id: String,
    pub model_id: String,
    pub dimension: String,
    pub digest: String,
    pub req_index: String,
    pub req_counts: String,
    pub tok_index: String,
    pub tok_counts: String,
}

/// `redis-store.ts:dimensionKeys(instanceId, modelId)`。
///
/// 摘要用 [`ratelimit_core::digest::sha256_hex`]（根 crate 的手写实现），
/// 与 TS 的 `createHash("sha256")` 对齐——两处必须是同一算法的同一输出，
/// 否则键前缀对不上、限流直接失效。
pub fn dimension_keys(instance_id: &str, model_id: &str) -> DimensionKeys {
    let dimension = crate::dimension::dimension_field(instance_id, model_id);
    let digest = ratelimit_core::digest::sha256_hex(dimension.as_bytes())[..32].to_string();
    DimensionKeys {
        instance_id: instance_id.to_string(),
        model_id: model_id.to_string(),
        req_index: format!("{KEY_PREFIX}req:{digest}:idx"),
        req_counts: format!("{KEY_PREFIX}req:{digest}:cnt"),
        tok_index: format!("{KEY_PREFIX}tok:{digest}:idx"),
        tok_counts: format!("{KEY_PREFIX}tok:{digest}:cnt"),
        dimension,
        digest,
    }
}

/// `RateLimitConfig`（`types.ts:141`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Input tokens per rolling minute. 0 disables the limiter.
    pub tpm: i64,
    /// Requests per rolling minute, on the same instance × model dimension.
    pub qpm: i64,
}

/// `redis-store.ts:degradedDecision()` — Redis 缺席时的 fail-open 决策。
///
/// 这条路径**允许请求通过**（`allowed: true`）并标记 `degraded`，
/// 由 `guard.enforceRateLimit` 负责节流告警（30s 一次）。剩余额度按配置
/// 全额给出，即"限流器不可用时视作没有消耗"。
pub fn degraded_decision(config: &RateLimitConfig, reason: &str) -> Decision {
    Decision {
        allowed: true,
        degraded: Value::Bool(true),
        degraded_reason: Some(reason.to_string()),
        reason: None,
        tpm: Value::from(config.tpm),
        qpm: Value::from(config.qpm),
        used_tokens: Value::from(0),
        used_requests: Value::from(0),
        remaining_tokens: Value::from(config.tpm),
        remaining_requests: Value::from(config.qpm),
        retry_after_seconds: Value::from(0),
    }
}

/// CHECK 请求的 eval 载荷：`checkRequest` 传给 Redis 的 KEYS/ARGV。
///
/// `redis-store.ts:checkRequest` 的调用形态是
/// `eval(CHECK_REQUEST_LUA, 6, configKey, overridesKey, reqIndex, reqCounts,
/// tokIndex, tokCounts, dimension, tpm, qpm, WINDOW_SECONDS)`。
/// 这里把它结构化成可断言的数据；`script_digest` 钉住脚本身份。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvalCall {
    pub op: String,
    pub script_digest: String,
    pub num_keys: u32,
    pub keys: Vec<String>,
    pub argv: Vec<String>,
}

/// `checkRequest` 的 KEYS/ARGV 编组。
///
/// TS 侧注意：`String(this.config.tpm)` / `String(this.config.qpm)`——
/// JS 数值转字符串。tpm/qpm 在配置里是 `number`，整数路径无小数点。
pub fn check_request_eval(
    keys: &DimensionKeys,
    config: &RateLimitConfig,
    script_digest: &str,
) -> EvalCall {
    EvalCall {
        op: "eval".to_string(),
        script_digest: script_digest.to_string(),
        num_keys: 6,
        keys: vec![
            format!("{KEY_PREFIX}config"),
            format!("{KEY_PREFIX}overrides"),
            keys.req_index.clone(),
            keys.req_counts.clone(),
            keys.tok_index.clone(),
            keys.tok_counts.clone(),
        ],
        argv: vec![
            keys.dimension.clone(),
            js_number_to_string(config.tpm as f64),
            js_number_to_string(config.qpm as f64),
            WINDOW_SECONDS.to_string(),
        ],
    }
}

/// `recordInputTokens` 的 KEYS/ARGV 编组。
///
/// TS：`String(Math.floor(inputTokens))`——**先 floor 再转字符串**。
/// 因此 `42.7` 记录成 `"42"`，`0.9` 记录成 `"0"`，负数 `-0.5` 记录成 `"-1"`
/// （`Math.floor` 向负无穷）。注意 `inputTokens <= 0` 在到达这里之前就已
/// 提前返回（`redis-store.ts:196`），所以实际只会出现正整数路径。
pub fn record_tokens_eval(
    keys: &DimensionKeys,
    input_tokens: f64,
    script_digest: &str,
) -> EvalCall {
    EvalCall {
        op: "eval".to_string(),
        script_digest: script_digest.to_string(),
        num_keys: 2,
        keys: vec![keys.tok_index.clone(), keys.tok_counts.clone()],
        argv: vec![
            WINDOW_SECONDS.to_string(),
            js_number_to_string(input_tokens.floor()),
        ],
    }
}

/// `getLimits()` 的 Hash 回读规则：字段缺失（`null`/空串）时回退到配置值。
///
/// TS：`raw[0] ? Number(raw[0]) : this.config.tpm`——注意这是**真值判断**，
/// 所以 `""`（空串）也走回退分支（JS 里 `""` 为 falsy）。
pub fn limits_from_hash(
    tpm_raw: Option<&str>,
    qpm_raw: Option<&str>,
    config: &RateLimitConfig,
) -> (i64, i64) {
    let pick = |raw: Option<&str>, fallback: i64| -> i64 {
        match raw {
            Some(s) if !s.is_empty() => js_number_to_int(s).unwrap_or(fallback),
            _ => fallback,
        }
    };
    (pick(tpm_raw, config.tpm), pick(qpm_raw, config.qpm))
}

/// Override 记录的解析：`{"tpm":n,"qpm":n}` / 裸数字 / 其他字符串。
///
/// 源：`getOverride` + `listOverrides`。三条分支（与 TS 一致）：
/// 1. JSON 对象 → 读字段，缺失字段回退配置；
/// 2. 其他可 JSON 解析值（如裸数字）→ `Number(parsed)`，qpm 回退配置；
/// 3. JSON.parse 抛错 → `Number(raw)`，qpm 回退配置。
pub fn parse_override(raw: &str, config: &RateLimitConfig) -> Option<(i64, i64)> {
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Object(map)) => {
            let tpm = map
                .get("tpm")
                .and_then(js_number_coerce)
                .unwrap_or(config.tpm);
            let qpm = map
                .get("qpm")
                .and_then(js_number_coerce)
                .unwrap_or(config.qpm);
            Some((tpm, qpm))
        }
        Ok(other) => {
            let tpm = js_number_coerce(&other)
                .unwrap_or_else(|| js_number_coerce_str(raw).unwrap_or(config.tpm));
            Some((tpm, config.qpm))
        }
        Err(_) => Some((js_number_coerce_str(raw).unwrap_or(config.tpm), config.qpm)),
    }
}

// ── JS 数值语义（本模块内部用；与 usage.rs / display.rs 的规则一致） ──

/// 与 `display::js_number_to_string` 同源：整数值不带 `.0`。
fn js_number_to_string(n: f64) -> String {
    crate::display::js_number_to_string(n)
}

/// `Number(string)` 的整数路径（用于 hash / override 回读）。
fn js_number_to_int(s: &str) -> Option<i64> {
    js_number_coerce_str(s)
}

/// `Number(value)` 对 JSON 值的强制转换；非数值返回 None。
fn js_number_coerce(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_f64().map(|f| f as i64),
        Value::String(s) => js_number_coerce_str(s),
        Value::Bool(b) => Some(if *b { 1 } else { 0 }),
        _ => None,
    }
}

/// `Number(string)`：空串与纯空白串得 0，`"0x10"` 得 16，其余按 JS 规则。
fn js_number_coerce_str(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0);
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(hex, 16).ok();
    }
    t.parse::<i64>().ok().or_else(|| {
        t.parse::<f64>()
            .ok()
            .filter(|f| f.is_finite())
            .map(|f| f as i64)
    })
}
