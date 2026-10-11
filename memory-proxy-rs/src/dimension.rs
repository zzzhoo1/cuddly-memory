//! dimension.rs — `redis-store.ts:324` `dimensionField` 的保真移植。
//!
//! 源实现只有一行：`return JSON.stringify([instanceId, modelId])`。
//! 移植的难点不在逻辑而在 **JSON.stringify 的字符串转义规则**——
//! 维度串里会出现任意用户可控的 `instanceId`/`modelId`（见 golden 的
//! `with"quote`、`back\slash`、`line\nbreak`、`模型-🚀` 用例）。
//!
//! JSON.stringify 转义表（ECMA-262 JSON.stringify，与 serde_json 差异点）：
//! - `"`  → `\"`      `\` → `\\`
//! - `\b` `\f` `\n` `\r` `\t` → 短转义
//! - 其他 < 0x20 → `\u00XX`（四位小写十六进制）
//! - 非 ASCII（中文、emoji 等）**原样输出 UTF-8，不转义**——serde_json
//!   默认同样不转义非 ASCII，一致。
//! - 单引号 `'`、斜杠 `/` 不转义。
//!
//! 这里不复用 serde_json::to_string，因为 golden 断言的是精确字节；
//! 手写转义并把差异点钉在注释里，行为可审。

/// `redis-store.ts:324`。输入含 NUL 等控制字符时按上表转义，
/// 输出恒为合法 JSON 数组字面量的字节。
pub fn dimension_field(instance_id: &str, model_id: &str) -> String {
    let mut out = String::with_capacity(instance_id.len() + model_id.len() + 5);
    out.push('[');
    push_json_string(&mut out, instance_id);
    out.push(',');
    push_json_string(&mut out, model_id);
    out.push(']');
    out
}

fn push_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}
