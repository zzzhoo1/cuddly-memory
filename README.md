# TencentDB-Agent-Memory — Rust 保真移植（rate-limit 核心）

将 `TencentDB-Agent-Memory` 仓库中 `MemoryProxy` 的**确定性核心**等价移植到 Rust。
验证方法：不在 Rust 侧重推导规格，而是用 `tsx` 直接执行参考 TS 实现产出 golden
数据，Rust 测试对 golden 做 diff（各 crate 测试文件头部有详细说明）。

## crate 结构（三个独立 crate，各自 lock / target）

| 目录 | crate | 内容 |
|---|---|---|
| `.`（根） | `ratelimit-core` | outbound-filter 插件语义骨架：消息值提取 / 规范化 JSON / SHA-256 摘要 / 请求体构建。手写 SHA-256（不引外部 digest crate）。 |
| `memory-proxy-rs/` | `memory-proxy-rs` | `MemoryProxy/src/rate-limit` 核心：`buildRateLimitResponse`（双协议）、`getActualInputTokens` / `numberField`、`dimensionField` 的保真移植。golden：usage 28 案例 + 429 responses 10 案例（含 wire 字节断言）+ dimension 6 案例。 |
| `rust-port/` | `anthropic-ratelimit-port` | anthropic 协议半边独立实现 + wire 字节断言（`tests/anthropic_golden.rs`）。 |

## Golden 数据（字节可复现）

- `golden.json` 是唯一真源：由 `gen-golden.mts` 用 `tsx` 直接运行参考实现产出
  （usage 28 / responses 10 / dimension 6）。脚本经 `MEMORYPROXY_REPO` 指向上游
  检出，默认 `/var/minis/shared/tdba/repo/MemoryProxy`。
  **不要手工编辑 `golden.json`**——只存在于该文件、生成器产不出的条目会破坏
  可复现性，CI 会当场把它比对出来。
- 四个嵌入副本（根目录 `golden_decisions_openai.json`、`golden_responses_openai.json`；
  `rust-port/` 下两个 anthropic 副本）由 `scripts/split-golden.mjs` 从 `golden.json`
  派生，供各 crate `include_str!` 嵌入。
- 重新生成：

  ```bash
  # 参考实现的 import 链会解析到检出目录的父级 node_modules，ioredis 需可解析：
  #   npm install --prefix <MemoryProxy 的父目录> --no-save --no-package-lock ioredis@5.11.1
  MEMORYPROXY_REPO=<...>/MemoryProxy npx tsx@4.23.15 gen-golden.mts > golden.json
  node scripts/split-golden.mjs
  ```

- 上游 provenance：`TencentCloud/TencentDB-Agent-Memory` @
  `8b86874a2daea49e3ff0fb53d699203146c5c77d`（MemoryProxy 子目录）。CI 的
  `golden-repro` 任务从该 commit 的新鲜检出重新生成并做字节比对——提交物与参考
  实现之间的对应关系由 CI 强制，而非依赖开发者自觉。

## CI（GitHub Actions）

构建与测试全部在 CI 执行（本地不加编译步骤）。push / PR 触发三个任务：

| 任务 | 内容 |
|---|---|
| `test` | 三个 crate 分别 `cargo test --locked` |
| `lint` | `cargo fmt --check`；`cargo clippy --all-targets -- -D warnings` |
| `golden-repro` | 取 pinned 上游 commit → `ioredis@5.11.1` → `tsx@4.23.15` 重生成 → 与 `golden.json` 字节比对 → `scripts/split-golden.mjs --check` |

工具链：CI 使用 GitHub Actions 的 stable。
