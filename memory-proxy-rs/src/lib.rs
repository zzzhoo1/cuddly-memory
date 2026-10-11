//! memory-proxy-rs — TencentDB-Agent-MemoryProxy 确定性核心的保真 Rust 移植。
//!
//! 参考实现（TypeScript，禁止改动）：
//!   `/var/minis/shared/tdba/repo/MemoryProxy/src/rate-limit/{guard,usage,redis-store}.ts`
//!
//! Golden 数据：`/var/minis/workspace/rust-port-poc/golden.json`，由
//! `gen-golden.mts` 用 **tsx 直接执行真实 TS 实现** 产出——Rust 侧对标的是
//! 真实行为而非转译者重新推导的规格（规格重推导会与转译共享同一类错误）。
//!
//! # 本 crate 覆盖（确定性纯函数层）
//!
//! | TS 源 | 函数 | 本 crate |
//! |---|---|---|
//! | `guard.ts:96` | `buildRateLimitResponse(protocol, decision)` | [`guard::build_rate_limit_response`]（双协议） |
//! | `usage.ts:7` | `getActualInputTokens(usage, protocol)` | [`usage::get_actual_input_tokens`] |
//! | `usage.ts:19` | `numberField(value)` | [`usage::number_field`]（JS `Number()` 语义） |
//! | `redis-store.ts:324` | `dimensionField(instanceId, modelId)` | [`dimension::dimension_field`] |
//! | `redis-store.ts:148` | `dimensionKeys()`（sha256 → 32 hex 命名空间） | [`store::dimension_keys`] |
//! | `redis-store.ts:301` | `degradedDecision()`（fail-open） | [`store::degraded_decision`] |
//! | `redis-store.ts:162` | `checkRequest` 的 KEYS/ARGV 编组 | [`store::check_request_eval`] |
//! | `redis-store.ts:198` | `recordInputTokens` 的 KEYS/ARGV 编组 | [`store::record_tokens_eval`] |
//! | `redis-store.ts:225` | `getLimits()` 回退规则 | [`store::limits_from_hash`] |
//! | `redis-store.ts:246` | override 解析（对象/裸值/非 JSON） | [`store::parse_override`] |
//!
//! # 显式不在范围（网络/IO 层，无 golden 可 diff）
//!
//! - `redis-store.ts` 的 Redis **连接与 eval 往返**（ioredis 连接池、重试）。
//!   键派生与调用编组已在 [`store`] 覆盖，Lua 脚本身份由 `scriptDigest` 钉住。
//! - `redis-store.ts` 的 `listOverrides()` **排序**：TS 用 `localeCompare`，
//!   结果随 ICU/默认 locale 变化（实测与码点序在 `a1`/`A1` 等处分歧），
//!   不是环境无关的确定性面。
//! - `guard.ts:21` 的 `enforceRateLimit`（组合 store 与 builder 的编排层）。
//!
//! # 方法论（从 POC `rust-port/` 验证后沿用）
//!
//! 1. **JS 数值语义保真**：`Number()`/`Math.max`/`Math.floor`/`String(n)`
//!    逐条对照 TS 源确认（含 "地板是 1 不是 0"、"fractional 保持小数"）。
//! 2. **wire 字节序**：golden 的 body 经 `canon()` 键排序，无法表达线上字节
//!    顺序；因此 body 用结构化断言（`serde_json::Value` 相等），另立
//!    wire-bytes 断言（按 guard.ts 对象字面量插入顺序手写）双轨验证。
//! 3. **golden 局限知情**：`canon()` 把 `NaN`/`Infinity` 数字序列化成字符串，
//!    golden 里 `usage.prompt_tokens: "NaN"` 实际对应 TS 运行时的数字 NaN
//!    输入；Rust 测试喂入的是字符串路径，两分支结果同为 0，结论仍成立
//!    （详见 `tests/golden_usage.rs` 注释）。

pub mod decision;
pub mod dimension;
pub mod display;
pub mod guard;
pub mod store;
pub mod usage;

pub use decision::{Decision, Protocol};
pub use dimension::dimension_field;
pub use display::js_number_to_string;
pub use guard::{build_rate_limit_response, BuiltResponse};
pub use store::{
    check_request_eval, degraded_decision, dimension_keys, limits_from_hash, parse_override,
    record_tokens_eval, DimensionKeys, EvalCall, RateLimitConfig, KEY_PREFIX, WINDOW_SECONDS,
};
pub use usage::{get_actual_input_tokens, number_field, to_f64};
