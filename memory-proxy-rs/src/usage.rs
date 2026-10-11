//! usage.rs — `usage.ts` 的保真移植：上游 usage → 总输入 token 归一化。
//!
//! 参考实现（TypeScript，禁止改动）：`usage.ts:7` `getActualInputTokens`。
//! 语义来源（文件头注释）：OpenAI `prompt_tokens` 已含缓存输入；Anthropic
//! `input_tokens` 不含，需加回 cache_read 与 cache_creation 两项。
//!
//! 返回类型是 `f64` 而非整型：TS 返回 JS number，golden 有 `1e21` 用例
//! （`max(0, floor(1e21)) === 1e21`），任何整型落点都会饱和失真。

use serde_json::Value;

/// `usage.ts:7`。`None`/缺字段按 0 处理；协议决定读哪组字段。
pub fn get_actual_input_tokens(usage: Option<&Value>, protocol: &str) -> f64 {
    let Some(u) = usage else { return 0.0 };
    if protocol == "openai" {
        return number_field(u.get("prompt_tokens"));
    }
    number_field(u.get("input_tokens"))
        + number_field(u.get("cache_read_input_tokens"))
        + number_field(u.get("cache_creation_input_tokens"))
}

/// `usage.ts:19` `numberField` — JS `Number()` 语义的显式移植。
///
/// 逐分支对照源码：
/// - `typeof === "number" && Number.isFinite` → `max(0, floor(v))`
///   （NaN/±Infinity 有 isFinite 检查拦截 → 0）
/// - `typeof === "string"` → `Number(v)`（**注意**：JS `Number("") === 0`、
///   `Number("  42 ") === 42`、`Number("0x2A") === 42`——这是 `Number` 全局
///   函数语义，不是 `parseInt`），isFinite 再过滤 → `max(0, floor)`
/// - 其余类型（object/array/null/bool/undefined）→ 0
pub fn number_field(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => {
            let f = n.as_f64().unwrap_or(f64::NAN);
            if f.is_finite() {
                f64::max(0.0, f.floor())
            } else {
                0.0
            }
        }
        Some(Value::String(s)) => match js_number_parse(s) {
            Some(f) if f.is_finite() => f64::max(0.0, f.floor()),
            _ => 0.0,
        },
        _ => 0.0,
    }
}

/// JS 全局 `Number(string)`：trim 空白、支持十六进制字面量、空串为 0。
/// 不支持八进制字面量（`"017"` → 17，不是 15）。
/// 返回 None 对应 TS 侧 `Number.isFinite(parsed) === false`（NaN 路径）。
fn js_number_parse(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return Some(0.0);
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        // JS: "0x" 后必须全为 hex 数字且不带符号，否则 NaN
        return if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            u64::from_str_radix(hex, 16).ok().map(|v| v as f64)
        } else {
            None
        };
    }
    // "Infinity" 是 Number() 的合法输入（不是 NaN）
    if t == "Infinity" || t == "+Infinity" {
        return Some(f64::INFINITY);
    }
    if t == "-Infinity" {
        return Some(f64::NEG_INFINITY);
    }
    // Rust f64 解析覆盖 JS 十进制语法主体（含 "1."、".5"、"1e5"、"+5"）
    t.parse::<f64>().ok()
}

/// `Number(v)` 的值域形态（guard builder 对 decision 数值字段用）。
/// 非有限数回退 0.0（对应 numberField 对不可用输入的处置），有限数原样。
pub fn to_f64(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}
