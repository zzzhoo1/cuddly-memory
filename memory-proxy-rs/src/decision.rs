//! decision.rs — `RateLimitDecision` 与 `RateLimitProtocol` 的 Rust 侧类型。
//!
//! 源：`redis-store.ts` 的 `RateLimitDecision` 接口 + `guard.ts` 的
//! `RateLimitProtocol`。字段全部建模为 `serde_json::Value`（JS 侧唯一类型
//! 是 number，但 golden 已证明边界输入会以奇怪形态到达——POC 阶段
//! `degraded: new Error()` 被 canon 成 `{}`；把数值字段同样放开是为了让
//! "转译器对输入形态做了什么假设"显式化，而不是藏进类型里）。
//!
//! builder 对这些字段的全部操作是 `Number(v)`（见 `usage::to_f64`）与
//! `String(n)`（见 `guard::js_number_to_string`），`Value` 足以承载。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// `guard.ts:7` — `export type RateLimitProtocol = "openai" | "anthropic"`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Openai,
    Anthropic,
}

/// `redis-store.ts` 的 `RateLimitDecision`。
///
/// `degraded` 是不透明 `Value`：golden 里为 `false`（正常路径）或 `{}`（
/// canon 后的 `new Error()`）。builder 从不读它（POC lib.rs 规则 3）。
///
/// `reason` 是 `"tpm" | "qpm" | null`——degraded 路径下为 `null`（见
/// `storeOps[1].check`），所以是 `Option<String>`。guard 层只在
/// `=== "qpm"` 时给 qpm 文案，`None` 与任何其他值走同一分支。
///
/// `degradedReason` 只在 degraded 时出现（`redis_unavailable` 或错误消息）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub allowed: bool,
    pub degraded: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub degraded_reason: Option<String>,
    pub reason: Option<String>,
    pub tpm: Value,
    pub qpm: Value,
    pub used_tokens: Value,
    pub used_requests: Value,
    pub remaining_tokens: Value,
    pub remaining_requests: Value,
    pub retry_after_seconds: Value,
}
