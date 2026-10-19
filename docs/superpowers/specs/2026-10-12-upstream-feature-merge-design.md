# 上游作者功能合并设计（CodexManagerLocal）

- 日期：2026-10-12
- 分支：`codexmanagerlocal-v062-integration`
- 上游：`origin` = `qxcnm/Codex-Manager`（作者）
- 本地 fork：`local` = `xiaozhang0801/codex-mangerlocal`
- 合并基线：v0.6.2（`09ba93db`，2026-09-25）

## 1. 目标与约束

**目标**：把作者在 v0.6.2 之后的「非赞助商」新功能合并进本 fork。

**硬约束**：
- 不得破坏本 fork 的自研功能（见第 2 节）。
- 赞助商条目、README 赞助内容、与本 fork 发布链冲突的 release 脚本一律不合并。
- 版本号 0.6.3 已由本 fork 自行 bump，不重复。
- 每批合并后必须实跑验证（`cargo test` + 相关前端 build/test），用输出证明未影响自研功能。

## 2. 必须保住的自研功能（基线盘点）

| 领域 | 功能 | 关键文件 |
|---|---|---|
| 网关 | 账号/IP 并发上限配置 | `gateway/upstream/proxy_pipeline/candidate_executor.rs`、`request_gate.rs`、`app_settings/*` |
| 网关 | 放宽同键+路径+模型并发上限 | `candidate_executor.rs` |
| 网关 | 可信客户端 IP 传递 | `client_ip.rs`、`gateway/local_validation/*` |
| 网关 | 实时请求活动生命周期 | `gateway/observability/request_activity.rs` |
| 应用 | 内网 IP 用量统计 + 排序 + 本周汇总 | `requestlog/requestlog_client_ip_usage.rs`、`apps/src/app/logs/*`、`dashboard.rs` |
| 应用 | 实时请求统计面板 | `apps/src/hooks/useDashboardActiveRequests.ts`、`dashboard.rs` |
| 存储 | SQLite 迁移列缺失兼容修复 | `storage` 迁移兼容逻辑 |
| 发布/CI | CodexManagerLocal 改名 + 指向本仓库的更新链路 | `.github/`、release 脚本 |

## 3. 方案选择

采用**方案 A：分批 cherry-pick**（只挑非赞助商提交，按依赖拓扑排序，逐批解冲突并验证）。

- 否决「整体 merge」：爆炸半径大，会拖入赞助/README/发布改动，破坏自研功能难定位。
- 否决「全手工移植」：payload 重构数百行新原语易漏。

## 4. 执行批次（依赖核查更正版）

> 更正说明：逐提交核查文件改动后发现，作者这批提交并非五块干净独立。`#494` 实际新建迁移 142 + `request_log_payload_purge.rs`，是 payload 清理地基，与 #495/#496/#498 同链，**已从「现在做」移入 payload 延后批**。`#491` 同时改了尚未引入的 `request_log_spill/segment.rs`、`#492` 尾部带一个尚不存在的 `request_log_payload_pipeline_tests.rs` hunk，均需拆分，payload 相关尾巴延后。

**本轮执行批次 1–3（均不触及 payload 存储层）；payload 延后批在 1–3 完成并验证后，单独一轮再做。**

### 批次 1 — 干净独立三提交（低冲突）
- `#487`（`fabd7ba3`）propagate upstream stream termination to chat completions clients — 仅 `chat_completions.rs`
- `#493`（`161ba792`）lazy last_insert_rowid + pin connection to one SQLite handle — 仅 `rusqlite/lib.rs`+`ci.yml`
- `#464`（`7c22c7b8`）make Codex auth cleanup opt-in — 设置项 + `codex_profile.rs`；**丢弃 README hunk**
- 价值：#487 对症线上 `stream closed before response.completed`；#493 连接级状态修复；#464 认证清理改可选。
- 冲突面：#487/#493 本 fork 未改动；#464 的 `app_settings/*`、`lib.rs`、i18n 与自研并发配置同文件，按新增字段处理。

### 批次 2 — 账号按模型资格过滤（中冲突）
- `#492`（`9d03ca83`）filter accounts by per-model qualification before sticky selection
- **跳过 hunk**：`request_log_payload_pipeline_tests.rs`（该测试文件属 payload 系列，当前不存在）。
- 冲突面：与自研并发网关共改 `candidate_executor.rs`。
- 共存策略：**资格过滤在前（筛选可用账号）→ 自研并发限流在后（限制并发数）**，两者职责不同，顺序串联，不互相覆盖。

### 批次 3 — passthrough 流式中断透传（中冲突）
- `#491`（`e6775a51`）propagate passthrough stream errors（gateway 部分）
- **拆分延后**：`request_log_spill/segment.rs` + 其 tests（35 行，依赖 #495 引入的 spill 模块，移入 payload 延后批）。
- 价值：对症线上 passthrough `broken pipe` / 提前断流。
- 冲突面：`stream_readers/*`、`sse_frame.rs`，本 fork 未改动，风险低。

### payload 延后批 — payload/spill/purge 统一重构（最高冲突，1–3 完成后单独一轮）
- `#494`（`86aa0108`）batched purge + 迁移 142 + `request_log_payload_purge.rs` + 存储空间 UI 卡片
- `#495`（`70e551fb`）spill segment primitives（新建 `request_log_spill/`）
- `#496`（`3b16432d`）move payload capture onto bounded background write queue
- `#498`（`31bf5c31`）bound batched clear purges by generation（迁移 143）
- 批次 1/3 拆出的 payload/spill 尾巴（#491 的 segment、#492 的 pipeline 测试）在此一并补回。
- 冲突面：与自研 IP 用量/日志存储深度重叠（`request_logs.rs`、`storage/mod.rs`、`apps/src/app/logs/*`、`rpc_dispatch/requestlog.rs`）。
- 迁移编号：当前最高 141，作者引入 142/143，不撞号；执行前核对双方 139–141 一致后再追加。

## 5. 跳过项
- 赞助商增删改（OmniaKey/OneSay/Sidrune/Daitou/PatewayAI/AIXiamo 等）与 README 赞助内容。
- `fix: publish release notes from changelog`（`b2c68c18`）：触及 release 脚本，可能与本 fork 发布链冲突，默认跳过，如需单独评估。

## 6. 验证策略（每批必做）
- Rust/网关/存储改动：`cargo test --workspace`（或最窄相关 package 测试）。
- 网关/协议改动：覆盖 streaming / 非 streaming / tools / 两种 OpenAI 风格端点的回归。
- 前端改动：`pnpm -C apps run build`；涉及运行时行为加 `pnpm -C apps run test:runtime`。
- 每批完成后，针对第 2 节自研功能跑对应回归测试，确认未破坏。
- 任一验证步骤无法在当前环境执行时，记录确切命令与原因。

## 7. 回滚
- 每批独立 cherry-pick，若某批破坏自研功能，单独 `git reset`/`git revert` 该批即可，不影响其他批次。
