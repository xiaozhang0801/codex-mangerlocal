# 上游功能合并 批次 1–3 实现计划（CodexManagerLocal）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把作者 3 个干净独立提交 + 账号模型资格过滤 + passthrough 流式修复，分 5 个任务 cherry-pick 进本 fork，全程保住自研功能。

**Architecture:** 分任务 cherry-pick（非整体 merge）。每任务一提交，冲突逐文件手工解、始终保留自研逻辑，payload/spill 相关 hunk 一律拆出延后。每任务结束跑窄包测试 + 必要前端 build 验证，然后让 cherry-pick 完成提交。

**Tech Stack:** Rust workspace（`codexmanager-core` / `codexmanager-service` / `rusqlite`）、Next.js 前端（`apps/`，`pnpm`）、Git cherry-pick。

## Global Constraints

- 上游提交在 `origin/main`（已 fetch）；均为单亲提交，可直接 `git cherry-pick <sha>`，无需 `-m`。
- 禁止合并：赞助商/README 赞助内容；遇到 README hunk 一律丢弃（保留本 fork 版本）。
- payload/spill/purge 相关改动（`request_log_spill/*`、`request_log_payload_*`、迁移 142/143、`request_log_payload_pipeline_tests.rs`）本轮一律**不引入**，拆出留给 payload 延后批。
- 自研功能不可破坏：账号/IP 并发上限、可信客户端 IP、实时请求统计、内网 IP 用量（统计/排序/周汇总）、SQLite 迁移兼容。
- 中文文件改动后用 UTF-8 回读确认无乱码。
- 窄包测试命令：`cargo test -p codexmanager-service`、`cargo test -p codexmanager-core`、`cargo test -p rusqlite`；前端：`pnpm -C apps run build`、`pnpm -C apps run test:runtime`。
- 仅在用户要求时推送/打包；本计划不含推送步骤。

---
### Task 1: 批次1-#487 chat 流式中断透传（fabd7ba3）

**Files:**
- Modify: `crates/service/src/gateway/observability/http_bridge/stream_readers/chat_completions.rs`
- Test: `crates/service/src/gateway/observability/http_bridge/stream_readers/chat_completions_repair_tests.rs`（随 commit 带入）

**Interfaces:**
- Consumes: 无（仅本提交内的 stream_reader 逻辑）
- Produces: 无跨任务依赖

- [ ] **Step 1: 确认工作区干净**

Run: `git -C "c:/Users/ZhangHao/Desktop/codex-manage2/Codex-Manager" status --short`
Expected: 仅 `.gitignore` 的既有改动或空；无半途 cherry-pick。

- [ ] **Step 2: cherry-pick #487**

```bash
git cherry-pick fabd7ba3
```
Expected: 两个文件为上游未改动区，大概率干净落地。若冲突，仅可能在 `chat_completions.rs`。

- [ ] **Step 3: 解冲突（若有）**

规则：`chat_completions.rs` 本 fork 未改过，直接取上游版（`git checkout --theirs <file>` 后 `git add`）。确认未牵连任何 payload/spill 文件。

- [ ] **Step 4: 完成提交**

```bash
git cherry-pick --continue   # 有冲突解完时；无冲突则此步跳过
```

- [ ] **Step 5: 验证**

Run: `cargo test -p codexmanager-service --no-fail-fast`
Expected: 全绿，尤其 `chat_completions_repair_tests` 通过。失败则 `git cherry-pick --abort` 或 `git reset --hard HEAD~1` 回滚后排查。

---

### Task 2: 批次1-#493 rusqlite 连接级修复（161ba792）

**Files:**
- Modify: `crates/rusqlite/src/lib.rs`
- Modify: `.github/workflows/ci.yml`（仅新增 rusqlite 测试步骤）

**Interfaces:**
- Consumes: 无
- Produces: 修复 `last_insert_rowid` 懒读 + 连接钉死单一 SQLite handle，自研存储逻辑间接受益

- [ ] **Step 1: cherry-pick #493**

```bash
git cherry-pick 161ba792
```
Expected: `rusqlite/src/lib.rs` 本 fork 未改，干净落地。`ci.yml` 若与本 fork 的 CI 定制冲突，仅保留「新增 rusqlite 测试 job」那几行，其余保留本 fork 版本。

- [ ] **Step 2: 解 ci.yml 冲突（若有）**

规则：本 fork 的 CodexManagerLocal 发布/更新链路相关 job 一律保留；只并入上游新增的 rusqlite 测试调用。

- [ ] **Step 3: 完成提交**

```bash
git cherry-pick --continue   # 若有冲突
```

- [ ] **Step 4: 验证**

Run: `cargo test -p rusqlite`
Expected: 全绿（含新增 rowid 语义 shim 测试）。再跑 `cargo test -p codexmanager-core --no-fail-fast` 确认存储层未被连接改动影响。

---

### Task 3: 批次1-#464 Codex 认证清理 opt-in（7c22c7b8）

**Files:**
- Modify: `crates/service/src/app_settings/api/{current,mod,patch}.rs`、`app_settings/mod.rs`、`app_settings/shared.rs`
- Modify: `crates/service/src/codex_profile.rs`、`crates/service/src/lib.rs`
- Modify: `apps/src/app/settings/components/general-basics-card.tsx`、`apps/src/lib/api/normalize.ts`、`apps/src/lib/store/useAppStore.ts`、`apps/src/types/settings.ts`、`apps/src/lib/i18n/messages/{en,ko,ru}.ts`
- Test: `apps/tests/gateway-auth-cleanup.spec.ts`、`crates/service/tests/app_settings.rs`、`codex_profile_tests.rs`（随 commit 带入）
- **丢弃**: `README.md`、`docs/en/README.md`、`docs/zh-CN/README.md` 的 hunk

**Interfaces:**
- Consumes: 无
- Produces: 新增设置字段 `codexAuthCleanup`（opt-in 开关），与自研并发配置字段并存于同一 settings schema

- [ ] **Step 1: cherry-pick #464（保留本地冲突以便手工取舍）**

```bash
git cherry-pick 7c22c7b8
```
Expected: 冲突集中在 `app_settings/*`、`lib.rs`、i18n、`useAppStore.ts`、`types/settings.ts`（本 fork 并发配置同文件）。

- [ ] **Step 2: 丢弃 README hunk**

规则：对 `README.md`、`docs/*/README.md`，执行 `git checkout --ours <file>` 保留本 fork 版本，再 `git add`。

- [ ] **Step 3: 解设置 schema 冲突（新增并存，不覆盖）**

规则：在 `app_settings/shared.rs`、`api/current.rs`、`api/patch.rs`、`types/settings.ts`、`useAppStore.ts`、i18n 中，**保留本 fork 的并发配置字段**，并**追加**上游的认证清理字段，二者并列。任何处若上游删除/改写了本 fork 字段，一律以保留本 fork 字段为准。

- [ ] **Step 4: 完成提交**

```bash
git add -A && git cherry-pick --continue
```

- [ ] **Step 5: 验证（Rust + 前端）**

Run: `cargo test -p codexmanager-service --no-fail-fast`
Run: `pnpm -C apps run build`
Run: `pnpm -C apps run test:runtime`
Expected: 全绿；设置页能看到认证清理开关，且本 fork 并发配置项仍在。UTF-8 回读 `useAppStore.ts` 和改动的 i18n 关键片段确认无乱码。

---

### Task 4: 批次2-#492 账号按模型资格过滤（9d03ca83）

**Files:**
- Create: `crates/service/src/account/account_model_support.rs`（随 commit 带入）
- Modify: `crates/service/src/account/mod.rs`、`app_settings/env_overrides/catalog/items.rs`
- Modify: `crates/service/src/gateway/upstream/proxy_pipeline/{candidate_executor,request_setup,response_finalize,stream_preflight}.rs`、`upstream/support/candidates.rs`
- Modify: `gateway/observability/http_bridge/aggregate/{openai_responses_event,output_text}.rs`
- Modify: `docs/en/report/environment-and-runtime-config.md`、`docs/zh-CN/report/环境变量与运行配置说明.md`
- **跳过**: `crates/service/src/gateway/observability/tests/request_log_payload_pipeline_tests.rs`（payload 系列，当前不存在）

**Interfaces:**
- Consumes: 现有 `candidate_executor.rs` 的候选选择流程 + 本 fork 并发 gate
- Produces: 候选选择前的「按模型资格过滤」步骤；新增可配 env（见 catalog/items.rs）

- [ ] **Step 1: cherry-pick #492**

```bash
git cherry-pick 9d03ca83
```
Expected: 冲突主要在 `candidate_executor.rs`（本 fork 并发逻辑），外加缺失的 `request_log_payload_pipeline_tests.rs` 报「文件不存在」。

- [ ] **Step 2: 跳过 payload 测试 hunk**

规则：若报 `request_log_payload_pipeline_tests.rs` 冲突/新增，执行 `git rm --cached --ignore-unmatch <file>` 并删除工作区该文件（不引入 payload 测试）。

- [ ] **Step 3: 解 candidate_executor.rs 冲突（串联共存）**

规则：按共存策略——**先执行上游「按模型资格过滤候选账号」，再执行本 fork「账号/IP 并发限流 + 放宽同键路径模型并发」**。两段逻辑顺序串联：资格过滤产出可用账号集合 → 并发 gate 在该集合上限流。禁止用上游版本整体覆盖掉本 fork 的并发分支，也禁止反之。`request_setup.rs`、`candidates.rs` 等按上游新增处理，若与本 fork 可信 IP 传递相邻，保留本 fork 相邻逻辑。

- [ ] **Step 4: 完成提交**

```bash
git add -A && git cherry-pick --continue
```

- [ ] **Step 5: 验证（重点回归自研并发）**

Run: `cargo test -p codexmanager-service --no-fail-fast`
Expected: 全绿，尤其 `candidate_executor_tests`、`request_gate_tests`（本 fork 并发回归）、`candidates_tests` 全通过。若并发回归失败，说明共存解法错误，回到 Step 3 重解，不得删本 fork 测试。

---

### Task 5: 批次3-#491 passthrough 流式中断透传（e6775a51，拆分）

**Files:**
- Modify: `crates/service/src/gateway/observability/http_bridge/aggregate/sse_frame.rs`
- Modify: `crates/service/src/gateway/observability/http_bridge/stream_readers/{common,passthrough}.rs`
- Test: `.../stream_readers/passthrough_repair_tests.rs`（随 commit 带入）
- **拆出延后**: `crates/core/src/request_log_spill/segment.rs` 及其 `tests/segment_tests.rs`（依赖 #495 的 spill 模块）

**Interfaces:**
- Consumes: 无（gateway stream_reader 独立）
- Produces: passthrough SSE 的上游中断透传与终止边界保留

- [ ] **Step 1: cherry-pick #491（不自动提交，便于拆分）**

```bash
git cherry-pick -n e6775a51
```
Expected: gateway 四文件应用；`request_log_spill/segment.rs` 因模块不存在报「文件不存在/需合并」。

- [ ] **Step 2: 剔除 spill 段改动**

规则：对 `crates/core/src/request_log_spill/segment.rs` 与 `request_log_spill/tests/segment_tests.rs`，若被引入工作区则 `git rm -f --ignore-unmatch` 并从暂存区移除；确保本任务不新增 `request_log_spill/` 任何文件。记录该段将在 payload 延后批补回。

- [ ] **Step 3: 暂存 gateway 改动并提交**

```bash
git add crates/service/src/gateway/observability/http_bridge/
git status --short   # 确认暂存区只含 gateway 文件，无 request_log_spill
git commit -m "fix(gateway): passthrough 流式中断透传(移植 #491，spill 段延后)"
```

- [ ] **Step 4: 验证**

Run: `cargo test -p codexmanager-service --no-fail-fast`
Expected: 全绿，`passthrough_repair_tests` 通过。再 `cargo build -p codexmanager-core` 确认未因缺 spill 段编译失败。

---

## 批次收尾（全部任务后）

- [ ] 跑一次全量回归：`cargo test --workspace --no-fail-fast` + `pnpm -C apps run build` + `pnpm -C apps run test:runtime`
- [ ] 核对第 2 节自研功能：设置页并发配置项在、IP 用量统计/排序/周汇总在、实时请求面板在。
- [ ] 汇总本轮移植了哪些提交、哪些 hunk 延后，等待用户决定是否推送/进入 payload 延后批。

## Self-Review 记录

- Spec 覆盖：批次 1（#487/#493/#464）、批次 2（#492）、批次 3（#491）全部有对应任务；payload 延后批不在本计划范围（符合 spec）。
- 占位符：无 TBD/TODO；冲突解法均给了具体规则。
- 类型/命名一致：各任务引用的文件路径、包名、测试名均来自实际核查。
