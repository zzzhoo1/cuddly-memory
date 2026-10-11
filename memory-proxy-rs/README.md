# memory-proxy-rs

TencentDB-Agent-MemoryProxy（`/var/minis/shared/tdba/repo/MemoryProxy`）确定性核心的保真 Rust 移植。
**P3 错译零容忍**：所有移植行为以真实 TS 执行产出的 golden 数据为准（`../gen-golden.mts` 用 tsx 直接运行参考实现），不做规格重推导。

## 覆盖范围（golden 全绿 26/26）

| TS 源 | 函数 | golden 覆盖 |
|---|---|---|
| `rate-limit/guard.ts:96` | `buildRateLimitResponse(protocol, decision)` 双协议 429 构建 | 10 案例 + wire 字节断言 |
| `rate-limit/usage.ts:7` | `getActualInputTokens`（usage 归一化） | 28 案例 |
| `rate-limit/usage.ts:19` | `numberField`（JS `Number()` 语义） | 含 `"0x10"`/`" 7"`/`"\u2028"`/`1e21` 边界 |
| `rate-limit/redis-store.ts:324` | `dimensionField`（`JSON.stringify([a,b])` 转义） | 6 案例 |
| `rate-limit/redis-store.ts:148` | `dimensionKeys`（sha256 前 32 hex 命名空间） | 5 案例 |
| `rate-limit/redis-store.ts:162` | `checkRequest` 的 KEYS/ARGV 编组 | stub client 捕获 + Lua 脚本摘要 |
| `rate-limit/redis-store.ts:198` | `recordInputTokens` 的 KEYS/ARGV 编组 | `String(Math.floor(n))` 边界断言 |
| `rate-limit/redis-store.ts:301` | `degradedDecision`（fail-open） | 无 client 的完整决策 |
| `rate-limit/redis-store.ts:225` | `getLimits` 回退（真值判断，`""` 也回退） | 单测 |
| `rate-limit/redis-store.ts:246` | override 解析（对象/裸值/非 JSON 三支） | 单测 |

## 已钉死的语义陷阱（详见各源文件注释）

1. retry 头地板是 **1 不是 0**（`Math.max(1, ·)`），fractional 保持小数，无 upper clamp
2. `degraded` 不产生任何头/体字段（builder 从不读它）
3. `reason !== "qpm"` 一律 tpm 文案 / `input_tpm_exceeded`（`!==` 语义）
4. body wire 字节序由 guard.ts 对象字面量插入顺序决定，golden 的 `canon()` 键排序无法表达 → 结构断言 + 独立 wire 断言双轨；openai wire 已用真实 TS 执行交叉验证
5. `Number("") === 0`、`Number("0x2A") === 42`（全局 Number 语义，非 parseInt）；NaN/±Infinity → 0
6. `canon()` 把 undefined 键整键跳过、NaN/Infinity 数字渲染为字符串——golden 形态差异的处理方式记录在 `tests/golden_usage.rs`
7. store 的键命名空间是 **sha256 前 32 hex**（不是全 64）；Lua 脚本身份用 `scriptDigest` 钉住，上游改脚本时 `golden_store.rs` 会红（已用 TS 源独立复算验证）
8. `listOverrides()` 的 `localeCompare` **不可字节保真移植**：结果随 Node ICU / 默认 locale 变化，实测与码点序在 `a1`/`A1`、`_x`/`0a` 等处分歧 → 只移植解析，排序留给调用方
9. `recordInputTokens` 的 `String(Math.floor(n))` 是**向负无穷**取整：`-0.5` → `"-1"`（截断会给 `"0"`）

## 显式不在范围（无 golden 可 diff，后补）

- `RedisRateLimitStore` 的**网络往返**（ioredis 连接池/重试）；键派生与调用编组已覆盖
- `listOverrides()` 的 locale 排序（见上第 8 条：非环境无关语义）
- `enforceRateLimit` 编排层（fail-open 节流告警）
- MemoryProxy 主体的 injection/session/handler 管线

## 运行

CI（GitHub Actions）在 push 时执行三个 crate 的 `cargo test --locked`、`cargo fmt --check` /
`cargo clippy -- -D warnings`，并校验 `golden.json` 可由 pinned 上游重新生成（见仓库根 README）。

本地如需手动运行：

```bash
cargo test --manifest-path memory-proxy-rs/Cargo.toml
```

工具链：stable。本地已用 **rustc 1.99.0** 跑通全量（`cargo test --locked`、`cargo fmt --check`、
`cargo clippy --all-targets -- -D warnings` 均绿）；系统自带的 cargo 1.75 无法解析本 crate 的
lockfile v4 与新版依赖，勿用。
