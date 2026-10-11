# TencentDB-Agent-Memory — Rust 保真移植（rate-limit 核心）

将 `TencentDB-Agent-Memory` 仓库中 `MemoryProxy` 的**确定性核心**等价移植到 Rust。
验证方法：不在 Rust 侧重推导规格，而是用 `tsx` 直接执行参考 TS 实现产出 golden
数据，Rust 测试对 golden 做 diff（各 crate 测试文件头部有详细说明）。

## crate 结构（三个独立 crate，各自 lock / target）

| 目录 | crate | 内容 |
|---|---|---|
| `.`（根） | `ratelimit-core` | outbound-filter 插件语义骨架：消息值提取 / 规范化 JSON / SHA-256 摘要 / 请求体构建。手写 SHA-256（不引外部 digest crate）。 |
| `memory-proxy-rs/` | `memory-proxy-rs` | `MemoryProxy/src/rate-limit` 核心：`buildRateLimitResponse`（双协议）、`getActualInputTokens` / `numberField`、`dimensionField`，以及 `RedisRateLimitStore` 的确定性面（键派生 / eval 编组 / 降级决策）。golden：usage 28 + 429 responses 10（含 wire 字节断言）+ dimension 6 + storeKeys 5 + storeOps 2。 |
| `rust-port/` | `anthropic-ratelimit-port` | anthropic 协议半边独立实现 + wire 字节断言（`tests/anthropic_golden.rs`）。 |

## Golden 数据（字节可复现）

- `golden.json` 是唯一真源：由 `gen-golden.mts` 用 `tsx` 直接运行参考实现产出
  （usage 28 / responses 10 / dimension 6 / storeKeys 5 / storeOps 2）。脚本经 `MEMORYPROXY_REPO` 指向上游
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

三种门禁：push / PR 触发三个任务，另有本地等价命令供离线复现。

| 任务 | 内容 | 本地等价 |
|---|---|---|
| `test` | 三个 crate 分别 `cargo test --locked` | `cargo test --locked --offline --manifest-path <c>` |
| `lint` | `cargo fmt --check`；`cargo clippy --all-targets -- -D warnings` | 同左（加 `--offline`） |
| `golden-repro` | 取 pinned 上游 commit → `ioredis@5.11.1` → `tsx@4.23.15` 重生成 → 与 `golden.json` 字节比对 → `scripts/split-golden.mjs --check` | 需上游检出，见上方「重新生成」段 |
| `upstream-patches` | 取同一 pinned commit → 应用 `patches/*.patch` → 校验补丁符号落地 → 装最小依赖（`vitest` + `crc-32`）→ 跑补丁新增的测试 → **回退实现后必须失败**（防空转测试） | 见下方「上游补丁」段 |

## 上游补丁（`patches/`）

`MemoryCore` 的 L1 抽取修复无法直推上游（本机 clone 是 read-only），因此做成可重放的
patch 序列，由 CI 在 pinned SHA 的新鲜检出上应用并验证。这样补丁与 Rust 移植共享同一条
可复现标准：**谁都能验，不依赖某台机器的 node_modules**。

| 补丁 | 内容 |
|---|---|
| `01-l1-json-contract-and-shape-tolerance.patch` | 网关降级系统提示导致的 L1 静默全丢：新增 `extractOutermostJson`（引号感知平衡扫描，替换会锚定内层数组的贪婪正则）；`LLMRunParams.outputContract` 让每个纯文本任务各自复述输出契约；新增 11 个测试重放实测到的网关应答形态 |
| `02-memoryproxy-lockfile-cost-guard.patch` | 删掉锁文件里指向不存在 `packages/cost-guard` 的 `file:` 依赖——该残留让 `pnpm install --frozen-lockfile` 直接失败 |

反向验证是这套 CI 的关键：撤掉实现补丁、留下测试，**必须失败**（本地实测 8 failed / 3 passed）。
若反向也绿，说明测试没在测东西——那比没测试更糟。

本地复现：

```bash
mkdir -p /tmp/up && cd /tmp/up && git init -q . \
  && git remote add origin https://github.com/TencentCloud/TencentDB-Agent-Memory.git \
  && git fetch -q --depth 1 origin 8b86874a2daea49e3ff0fb53d699203146c5c77d \
  && git checkout -q FETCH_HEAD
for p in <repo>/patches/*.patch; do git apply "$p"; done
cd MemoryCore && npm i --no-save --no-fund --no-audit --ignore-scripts vitest@4.1.11 crc-32@1.2.2
npx vitest run src/utils/sanitize.test.ts src/core/record/l1-extractor.test.ts
```

本地验证（2026-10-11 实战记录）：rustc 1.99.0 下三个 crate 共 **42 个测试全绿**，
`fmt` / `clippy -D warnings` 零告警；生成器两次运行字节一致。安装工具链：

```bash
curl -sSL -o rustup-init https://static.rust-lang.org/rustup/dist/aarch64-unknown-linux-gnu/rustup-init
chmod +x rustup-init && RUSTUP_DIST_SERVER=https://mirrors.tuna.tsinghua.edu.cn/rustup \
  ./rustup-init -y --no-modify-path --default-toolchain stable --profile minimal
```

工具链：CI 使用 GitHub Actions 的 stable。
