# memory-proxy-rs

TencentDB-Agent-MemoryProxy（`/var/minis/shared/tdba/repo/MemoryProxy`）确定性核心的保真 Rust 移植。
**P3 错译零容忍**：所有移植行为以真实 TS 执行产出的 golden 数据为准（`../gen-golden.mts` 用 tsx 直接运行参考实现），不做规格重推导。

## 覆盖范围（golden 全绿 16/16）

| TS 源 | 函数 | golden 覆盖 |
|---|---|---|
| `rate-limit/guard.ts:96` | `buildRateLimitResponse(protocol, decision)` 双协议 429 构建 | 10 案例 + wire 字节断言 |
| `rate-limit/usage.ts:7` | `getActualInputTokens`（usage 归一化） | 28 案例 |
| `rate-limit/usage.ts:19` | `numberField`（JS `Number()` 语义） | 含 `"0x10"`/`" 7"`/`"\u2028"`/`1e21` 边界 |
| `rate-limit/redis-store.ts:324` | `dimensionField`（`JSON.stringify([a,b])` 转义） | 6 案例 |

## 已钉死的语义陷阱（详见各源文件注释）

1. retry 头地板是 **1 不是 0**（`Math.max(1, ·)`），fractional 保持小数，无 upper clamp
2. `degraded` 不产生任何头/体字段（builder 从不读它）
3. `reason !== "qpm"` 一律 tpm 文案 / `input_tpm_exceeded`（`!==` 语义）
4. body wire 字节序由 guard.ts 对象字面量插入顺序决定，golden 的 `canon()` 键排序无法表达 → 结构断言 + 独立 wire 断言双轨；openai wire 已用真实 TS 执行交叉验证
5. `Number("") === 0`、`Number("0x2A") === 42`（全局 Number 语义，非 parseInt）；NaN/±Infinity → 0
6. `canon()` 把 undefined 键整键跳过、NaN/Infinity 数字渲染为字符串——golden 形态差异的处理方式记录在 `tests/golden_usage.rs`

## 显式不在范围（无 golden 可 diff，后补）

- `RedisRateLimitStore`（Redis Lua、连接池）
- `enforceRateLimit` 编排层（fail-open 节流告警）
- MemoryProxy 主体的 injection/session/handler 管线

## 运行

CI（GitHub Actions）在 push 时执行三个 crate 的 `cargo test --locked`、`cargo fmt --check` /
`cargo clippy -- -D warnings`，并校验 `golden.json` 可由 pinned 上游重新生成（见仓库根 README）。

本地如需手动运行：

```bash
cargo test --manifest-path memory-proxy-rs/Cargo.toml
```

工具链：stable（cargo 1.75 无法解析本 crate 的 lockfile v4 与新版依赖，勿用）。
