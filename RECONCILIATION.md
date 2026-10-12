# 里程碑 1：补丁对账 + 用例/契约差集清单

- 日期：2026-10-12
- 方法：不碰 `src/`，只做只读核对。全部结论可按下文命令复现。
- 基线：本仓库 `733400d`（=origin/main，worktree 干净）；上游 pin `8b86874`（MemoryProxy@8b86874 子目录）；上游 HEAD `b1f79fc`（本次现场 fetch）。
- 当场验证（非引用记忆）：三 crate `cargo test --locked --offline` 全绿 = 11+26+5 = **42 测试**；`fmt --check` / `clippy -D warnings` 三 crate 全过；golden 重生成**字节一致**；`split-golden.mjs --check` 通过。工具链 rustup 1.99（系统 apt cargo 1.75 编不动本仓库，属已记录的环境事实）。

---

## 结论摘要

1. **补丁 01 对 pin 完全对账**（回放后 11/11 文件与作者本地树 sha256 全同），但**对 HEAD 出现了语义错位**：文件仍可 `git apply`，`standalone/llm-runner.ts` 已漂移。
2. **补丁 02 对 pin 落地确认，但对 HEAD 已失效**（`apply --check` FAIL）：HEAD 的锁文件残留还在，且形态不同；`cost-guard` 是**上游真实组件**（HEAD 有 `scripts/qa/cost-guard-smoke.sh`），本仓库的"清理"只是删了锁文件里的孤儿条目，上游对它没有自己修。
3. **上游在 pin→HEAD 之间发生了结构性漂移**：`MemoryProxy/src/__tests__/` 目录被整体删除；rate-limit 新增第三种协议 `"responses"`；`MemoryCore` 全面无单元测试（pin 与 HEAD 皆然），测试面转移为 `scripts/qa/*.sh|py`。
4. **oracle 覆盖面存在三块空洞**：①`getLimits`/`getOverride` 解析语义无 golden（gen-golden 未捕获 hmget/hget 路径）；②根 crate `ratelimit-core` 的**出处不可溯源**——上游 pin/HEAD 全树与全历史均无 "outbound-filter" 模块，11 个测试无参考实现可比；③golden 的 openai 两个 split 文件生成后无人消费。
5. **`localeCompare` 与 Redis 连接层**仍为显式排除项（无 golden 可 diff），此项与既有判断一致，无需变更。
6. **"全绿"的真实边界**：42 测试中 24 个由 golden 双重绑定（TS 直跑产物），其余为规格推导/手写断言。绿 ≠ 全保真，仅"被 golden 覆盖的面在 pin 上保真"。

---

## A. patches 对账

### A1. 补丁 01（`01-l1-json-contract-and-shape-tolerance.patch`）

| 项 | pin（8b86874） | HEAD（b1f79fc） |
|---|---|---|
| `git apply --check` | OK | **OK**（但见下） |
| 落点文件 11 个（8 改 + 3 新增） | 回放后与作者本地树 **11/11 sha256 相同** | 10/11 相同；**`MemoryCore/src/adapters/standalone/llm-runner.ts` 漂移**（pin `a38cfd57…` → head `e0d4ecf1…`） |
| 新增测试 2 文件（11 个 it） | 上游 pin 无同名文件（净新增） | 上游 HEAD 无同名文件；且**全上游 `MemoryCore` 无任何 `.test.ts`** |
| 上游原有测试面 | `MemoryProxy/src/__tests__/` 仅 2 个非 rate-limit 测试；MemoryCore 无测试 | **`__tests__/` 目录被删除**；测试面转为 `MemoryProxy/scripts/qa/`（12 个脚本） |

**风险**：`git apply --check` 通过只证明 hunk 上下文匹配。`standalone/llm-runner.ts` 漂移后，补丁注入的 `outputContract` 参数是否与新的上游签名一致，**静态上无法由 CI 现有的 grep 符号断言发现**（该断言只查补丁自己的符号存在）。

**复现**：
```bash
git -C /var/minis/shared/tdba/repo show 8b86874:MemoryCore/src/adapters/standalone/llm-runner.ts | sha256sum
sha256sum /tmp/uphead/MemoryCore/src/adapters/standalone/llm-runner.ts
git apply --check patches/01-*.patch   # 在 HEAD 检出上
```

### A2. 补丁 02（`02-memoryproxy-lockfile-cost-guard.patch`）

| 项 | pin | HEAD |
|---|---|---|
| 内容 | 删 `MemoryProxy/pnpm-lock.yaml` 中 3 处 `cost-guard@file:packages/cost-guard`（9 行） | 同左，但**补丁上下文已变**：HEAD 里条目为 `version: file:packages/cost-guard(cos-nodejs-sdk-v5@3.0.0)` |
| 应用 | 落地确认（`git apply --reverse --check` 通过）；但 **`git status` 不可见**——该文件被上游 `.gitignore:35` 的 `pnpm-lock.yaml` 规则忽略，而它**实际被上游跟踪**（pin/HEAD 树内均存在） | **`apply --check` FAIL**（`patch does not apply`，冲突于 `@@ -67,9 +67,6 @@`） |
| 上游现状 | 锁文件 6 行 cost-guard 残留；`packages/cost-guard` 目录**不存在**；`package.json` 无此依赖 | 残留**仍在**（6 行，形态更新）；`packages/` 仍不存在；但新增 `scripts/qa/cost-guard-smoke.sh`（引用 `packages/cost-guard`，假设它存在） |

**判断**：这不是"上游已修"——上游从未修，只是结构变了导致补丁失配。`cost-guard` 干净问题在上游 HEAD 上依然是一个悬而未决的仓库卫生问题。

**复现**：
```bash
git -C /var/minis/shared/tdba/repo show HEAD:MemoryProxy/pnpm-lock.yaml | grep -c cost-guard   # 6（pin=6）
cd /tmp/uphead && git apply --check .../patches/02-*.patch                                    # FAIL
grep -n 'cost-guard' /tmp/uphead/MemoryProxy/scripts/qa/cost-guard-smoke.sh
```

---

## B. 用例/契约差集

### B1. oracle 覆盖矩阵（Rust 42 测试）

| 面 | 测试数（约） | oracle | 依据 |
|---|---|---|---|
| usage 归一化 | 3 | ✅ golden `usage` 28 | gen-golden 直跑 `usage.ts` |
| 429 响应构造（双协议） | 10+ | ✅ golden `responses` 10 | 直跑 `guard.ts`；wire 字节断言另立 |
| dimensionField / 键派生 | 2+3 | ✅ golden `dimension` 6 / `storeKeys` 5 | 直跑 `redis-store.ts` |
| eval 编组（CHECK/RECORD） | 2 | ✅ golden `storeOps[0]` + Lua digest | 记录型 stub 捕获 |
| 降级决策 | 2 | ✅ golden `storeOps[1]` | 直跑（no client 分支） |
| **`getLimits` 回退** | 1 | ❌ **无**（手写） | gen-golden 未调用 hmget 路径 |
| **`getOverride`/`parse_override`** | 1 | ❌ **无**（手写） | gen-golden 未调用 hget 路径 |
| 根 crate 全部（digest/body/config） | 11 | ❌ **无**（手写；1 个已知向量可独立验证） | 参考实现不可溯源（见 B3） |
| rust-port（anthropic 半边） | 5 | ✅ 但**与 memory-proxy-rs 同 oracle 重复** | 同一 golden，两处实现 |

### B2. 契约面对照（上游 pin 的 rate-limit 三文件 = 496 行）

- **已有对应**：`getActualInputTokens`、`numberField`、`dimensionField`、`dimensionKeys`（sha256→32hex）、`buildRateLimitResponse`、`degradedDecision`、`checkRequest`/`recordInputTokens` 的 KEYS/ARGV 编组、两个 Lua 脚本 digest。
- **显式排除（既往判断，维持）**：`enforceRateLimit` 编排、`listOverrides` 排序、Redis 连接/eval 往返、`setLimits`/`deleteLimits` 等管理面。
- **未显式但未覆盖**：`isAvailable()`/`assertAvailable()` 的状态语义；`getOverride` 的 async+`assertAvailable` 组合行为。
- **上游自身双实现漂移（观察，非本仓库缺陷）**：同一 override 数据，Lua 内解析（`redis-store.ts:43-51`：`tonumber(...) or tpm`）与 TS `getOverride`（`:249-261`：`Number(value.tpm ?? config.tpm)`）**语义不同**——Lua 分支对 `{"tpm":"abc"}` 的行为与 TS 分支不同。Rust `parse_override` 对应的是 TS 半边；Lua 半边只固定了脚本 digest。建议后续单列 issue 决定跟哪半边。

### B3. 根 crate `ratelimit-core` 出处问题（最高优先级发现）

- 根 `Cargo.toml` 的 crate 名是 `ratelimit-core`，README 称移植自 "outbound-filter 插件"。
- **证据**：上游 pin 树、HEAD 树、全历史 `git log -S'outbound-filter'` 均无命中；`grep -r 'buildRateLimitBody\|mergeConfigs\|blankKeyMarker'` 在上游全树无命中；本机 pnpm store、`/root/.config/opencode`、`tdba/agents` 亦无命中。
- **含义**：该 crate 的 11 个测试**没有任何可核对的参考实现**，"移植/保真"声称缺乏依据；它要么出自未入库的私有插件（需补出处与版本），要么应改标注为"自研实现（规格推导）"。
- 注意：`digest.rs` 的已知向量（`sha256('{"a":1,"b":2}')`）可独立验证，非问题；问题在 `build_rate_limit_body` / `merge_configs` / `digest_message_value` 的**语义**无 oracle。

### B4. 杂项差集

| 项 | 状态 |
|---|---|
| split 副本 `golden_decisions_openai.json` / `golden_responses_openai.json` | **无消费者**（根/两个 crate 均不 `include_str!` 它们；memory-proxy-rs 直接读全量 `golden.json`）。`--check` 保证不漂移，但纯增维护面 |
| `golden/` 空目录 | 未跟踪，无内容 |
| `gen-err.log` | 已 .gitignore；工作树遗留文件 |
| `rust-port/` 与 `memory-proxy-rs/` 的 anthropic 双实现 | 同 oracle 重复覆盖，属冗余非缺陷 |
| 本机 `/etc/hosts` github pin | 仍在（`20.27.177.113`），属本机环境，不入库 |
| 本地 `Cargo.lock` 版本 | v3（cargo 1.75 生成）；rustup 1.99 可解析，CI `--locked` 通过 |

---

## C. 建议修正（按优先级，均为后续动作，本次未执行）

1. **补丁重基（对 HEAD）**：`02` 需按 HEAD 锁文件新形态重生成；`01` 需处理 `standalone/llm-runner.ts` 漂移。同时给 patch-replay CI 增加"对 HEAD 的 canary 步"——`git apply --check` 对 HEAD 失败即红（当前 CI 只回放 pin，发现不了这层）。
2. **补 oracle 空洞**：`gen-golden.mts` 的 stub 已实现 hmget/hget，只需新增两个调用场景（`getLimits` 空串回退、`getOverride` 三分支）即可让 `limits_from_hash`/`parse_override` 获得 golden；成本一个生成器小改 + golden 重生成。
3. **根 crate 出处澄清**：二选一——补出处（插件仓库/版本/commit）并把 oracle 指向它，或改标注为"自研（规格推导）"并在 README 去掉"移植"字样。
4. **清理**：删除两个无消费者的 openai split 文件（或让根 crate 消费它们），清空 `golden/`。
5. **里程碑 2 的前提**：上述 1–3 完成前，前滚演练的结论已可预期——**当前补丁集对 HEAD 不是可直接前滚状态**；建议演练以"重基后的补丁集"为对象，验收标准写死：对 HEAD `apply` 成功 ∧ 补丁测试在 HEAD 的依赖面上跑绿 ∧ 反向验证（撤实现）必红。

---

## 附：本次当场验证命令与结果

```bash
# 全绿复验（rustup 1.99）
cargo test --locked --offline --manifest-path Cargo.toml            # 11 passed
cargo test --locked --offline --manifest-path memory-proxy-rs/Cargo.toml  # 2+11+10+3 passed
cargo test --locked --offline --manifest-path rust-port/Cargo.toml  # 5 passed

# golden 可复现（pin 全树检出 + ioredis）
npm install --prefix /tmp/recon --no-save --no-package-lock --no-fund --no-audit ioredis@5.11.1
MEMORYPROXY_REPO=/tmp/recon/up/MemoryProxy npx --yes tsx@4.23.15 gen-golden.mts > /tmp/recon/golden.new.json
cmp /tmp/recon/golden.new.json golden.json    # 字节一致（19689 B）
node scripts/split-golden.mjs --check         # in sync

# 补丁回放对账（pin）
cd /tmp/recon/up && for p in patches/*.patch; do git apply --check "$p" && git apply "$p"; done
# 11/11 文件与 /var/minis/shared/tdba/repo 下作者树 sha256 相同

# 对 HEAD 的干跑
cd /tmp/uphead && git apply --check patches/01-*.patch   # OK
cd /tmp/uphead && git apply --check patches/02-*.patch   # FAIL
```
