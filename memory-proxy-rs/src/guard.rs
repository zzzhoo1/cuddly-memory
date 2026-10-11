//! guard.rs — `guard.ts:96` `buildRateLimitResponse` 的双协议保真移植。
//!
//! 已验证语义（对照 guard.ts + golden，POC 阶段钉死，此处沿用）：
//!
//! 1. 两个 retry 头是 `String(Math.max(1, retryAfterSeconds))`——地板 **1 不是 0**；
//!    `Math.max` 施加于 decision 字段本身，fractional 输入（如 1.9）保持
//!    1.9 → `String(1.9) = "1.9"`。无 upper clamp（86400 原样）。
//! 2. `degraded` 不产生任何头或体字段——builder 从不读它。
//! 3. 中文文案逐字节：`reason === "qpm"` → qpm 文案；**任何**其他 reason
//!    （`!==` 语义）→ tpm 文案。
//! 4. body wire 字节序由 guard.ts 对象字面量插入顺序决定：
//!    - anthropic：`{"type":"error","error":{"type":"…","message":"…"}}`
//!    - openai：`{"error":{"message":"…","type":"…","code":"…","param":null}}`
//!    golden 的 body 是 `canon()` 键排序渲染，只能做结构断言（rule 5）。
//! 5. openai 分支独有 `code` 字段：`qpm → "qpm_exceeded"`，其余 →
//!    `"input_tpm_exceeded"`（同样 `!==` 语义）。

use crate::decision::{Decision, Protocol};
use crate::display::js_number_to_string;
use crate::usage::to_f64;

/// TS `Response` 等价物。headers 为小写键（undici `Headers` 归一化），
/// `BTreeMap` 相等 = 键集合与值全等。
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltResponse {
    pub status: u16,
    pub headers: std::collections::BTreeMap<String, String>,
    /// TS `Response` body 的精确字节（`JSON.stringify(body)` 产物）。
    pub body_bytes: Vec<u8>,
}

const MSG_QPM: &str = "该模型请求频率已达上限，请稍后重试";
const MSG_TPM: &str = "该模型输入 Token 用量已达上限，请稍后重试";

fn message_for(reason: &str) -> &'static str {
    if reason == "qpm" {
        MSG_QPM
    } else {
        MSG_TPM
    }
}

/// `guard.ts:96`。双协议 429 构建，`status` 恒 429。
pub fn build_rate_limit_response(protocol: Protocol, decision: &Decision) -> BuiltResponse {
    let message = message_for(&decision.reason);

    let body_bytes = match protocol {
        Protocol::Anthropic => {
            // guard.ts:101-103 插入顺序
            format!(
                r#"{{"type":"error","error":{{"type":"rate_limit_error","message":"{message}"}}}}"#
            )
            .into_bytes()
        }
        Protocol::Openai => {
            // guard.ts:104-110 插入顺序：message, type, code, param
            let code = if decision.reason == "qpm" {
                "qpm_exceeded"
            } else {
                "input_tpm_exceeded"
            };
            format!(
                r#"{{"error":{{"message":"{message}","type":"rate_limit_error","code":"{code}","param":null}}}}"#
            )
            .into_bytes()
        }
    };

    let retry = to_f64(&decision.retry_after_seconds);
    let retry_header = js_number_to_string(f64::max(1.0, retry));

    let mut headers = std::collections::BTreeMap::new();
    headers.insert(
        "content-type".to_string(),
        "application/json; charset=utf-8".to_string(),
    );
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
