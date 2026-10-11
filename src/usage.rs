//! usage.rs — usage 头数值的 JS `Number()` 语义解析（`build_rate_limit_body` 用）。
//!
//! 与 `memory-proxy-rs/src/usage.rs` 的 `js_number_parse` 保持同一子集：
//! trim → 空串为 0 → `0x`/`0X` 前缀十六进制 → 其余按十进制（Rust f64 解析
//! 覆盖 JS 十进制语法主体，含 `1e3`、`.5`、`+5`）。非有限结果一律视为
//! 不可用（None），调用方 `filter_map` 跳过该字段——对应"仅当解析出可用
//! 数字才写入请求体"的语义。
//!
//! 子集说明：JS `Number()` 还接受 `0b`/`0o` 字面量与布尔/`null` 输入
//! （`Number(null) === 0`）；这些形态在 usage 头场景不会出现，本实现与
//! `memory-proxy-rs` 侧的既定语料一致，不额外展开。

use serde_json::Value;

/// JS `Number(v)` + 有限性检查：可用的数值返回 `Some`。
///
/// 仅接受 JSON number 与 string；其余类型（null/bool/对象/数组）→ `None`。
pub fn parse_number(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|f| f.is_finite()),
        Value::String(s) => {
            let t = s.trim();
            let parsed = if t.is_empty() {
                // JS: Number("") === 0
                Some(0.0)
            } else if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
                // JS: "0x" 后必须为至少一位 hex 数字，否则 NaN
                if !hex.is_empty() && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    u64::from_str_radix(hex, 16).ok().map(|n| n as f64)
                } else {
                    None
                }
            } else {
                t.parse::<f64>().ok()
            };
            parsed.filter(|f| f.is_finite())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn numbers_pass_through() {
        assert_eq!(parse_number(&json!(0)), Some(0.0));
        assert_eq!(parse_number(&json!(42.5)), Some(42.5));
        assert_eq!(parse_number(&json!(-3)), Some(-3.0));
        assert_eq!(parse_number(&json!(1e21)), Some(1e21));
    }

    #[test]
    fn strings_follow_js_number() {
        assert_eq!(parse_number(&json!("42")), Some(42.0));
        assert_eq!(parse_number(&json!("  42.5 ")), Some(42.5));
        assert_eq!(parse_number(&json!("-3")), Some(-3.0));
        assert_eq!(parse_number(&json!("")), Some(0.0));
        assert_eq!(parse_number(&json!("0x2A")), Some(42.0));
        assert_eq!(parse_number(&json!("0X2A")), Some(42.0));
        assert_eq!(parse_number(&json!("1e3")), Some(1000.0));
        assert_eq!(parse_number(&json!(".5")), Some(0.5));
    }

    #[test]
    fn unusable_values_are_none() {
        assert_eq!(parse_number(&json!("abc")), None);
        assert_eq!(parse_number(&json!("0x")), None);
        assert_eq!(parse_number(&json!("0xZZ")), None);
        assert_eq!(parse_number(&json!(null)), None);
        assert_eq!(parse_number(&json!(true)), None);
        assert_eq!(parse_number(&json!({})), None);
        assert_eq!(parse_number(&json!([1])), None);
    }
}
