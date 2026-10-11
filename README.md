# TencentDB-Agent-Memory — Rust 保真移植（rate-limit 核心）

将 `TencentDB-Agent-Memory` 仓库中 `MemoryProxy` 的**确定性核心**等价移植到 Rust。
验证方法：不在 Rust 侧重推导规格，而是用 `tsx` 直接执行参考 TS 实现产出 golden
数据，Rust 测试对 golden 做 diff（各 crate 测试文件头部有详细说明）。

## crate 结构（三个独立 crate，各自 lock / target）

| 目录 | crate | 内容 |
|---|---|---|
| `.`（根） | `ratelimit-core` | outbound-filter 插件语义骨架：消息值提取 / 规范化 JSON / SHA-256 摘要 / 请求体构建。手写 SHA-256（不引外部 digest crate）。 |
| `memory-proxy-rs/` | `memory-proxy-rs` | `MemoryProxy/src/rate-limit` 核心：`buildRateLimitResponse`（双协议）、`getActualInputTokens` / `numberField`、`dimensionField` 的保真移植。golden 测试：usage 28 案例 + 429 responses 10 案例（含 wire 字节断言）+ dimension 6 案例。 |
| `rust-port/` | `anthropic-ratelimit-port` | anthropic 协议半边独立实现 + wire 字节断言（`tests/anthropic_golden.rs`）。 |

## Golden 数据

- `golden.json`：由 `gen-golden.mts` 用 tsx 直接运行参考实现产出（usage / responses / dimension 三段）。
  重新生成需要上游检出位于 `/var/minis/shared/tdba/repo/MemoryProxy`。
- `golden_decisions_openai.json`、`golden_responses_openai.json`（根目录）与
  `rust-port/golden_decisions_anthropic.json`、`rust-port/golden_responses_anthropic.json`：
  golden.json 按协议拆分后的嵌入副本（`include_str!`）。

上游参考（provenance）：`TencentDB-Agent-Memory` 仓库 `8b86874a2daea49e3ff0fb53d699203146c5c77d`
（MemoryProxy 子目录 `9f2c78af856a4aa0ba0d7192dc15a7269092e84a`）。

## 构建 / 测试

构建由 CI（GitHub Actions，`.github/workflows/ci.yml`）在 push 时执行，三个 crate 分别：

```bash
cargo test --locked --manifest-path Cargo.toml
cargo test --locked --manifest-path memory-proxy-rs/Cargo.toml
cargo test --locked --manifest-path rust-port/Cargo.toml
```

工具链：CI 使用 stable。
