# ZhangHao 本地功能保留与迁移说明

本文档用于后续把 ZhangHao 本地分叉里的功能迁移到作者新版 `Codex-Manager` 项目中。内容基于当前本地仓库的 git 历史和源码梳理，比较范围是：

- 基线：`f3efb3a2`，当前本地 `upstream/main`
- 本地范围：`f3efb3a2..HEAD`
- 作者：`zhanghao <soft15@foshk.com>`
- 主要功能提交：
  - `83454653` `加入ip统计`
  - `22704466` `feat(dashboard): add desktop active request monitor`
  - `5a38f09f` `feat(apikeys): 增加内网 IP 今日 Token 统计`
  - `d0dc09a7` `feat(gateway): 增加按客户端 IP 的并发排队`
  - `f82a826c` `ci: add CodexManagerLocal release workflow`
  - `b8a60861` `ci: fix CodexManagerLocal config path`

不作为主要迁移目标的提交：`65c52efa` 样式优化、`a6adc7dd` 弹窗居中、`1c77b643` 长弹窗底部遮挡、`3768119e` 本地 agent 产物清理。

## 功能总览

| 功能 | 保留价值 | 主要落点 |
| --- | --- | --- |
| 客户端 IP 识别、请求日志、Token 统计 | 能看出内网哪台机器/哪个 API Key 在消耗 Token | `crates/service/src/client_ip.rs`、`request_logs`、`request_token_stats`、`requestlog/client_ip_usage`、API Keys 页面 |
| Dashboard 实时活跃请求监控 | 管理员能看到正在运行和排队的请求 | `gateway/observability/request_activity.rs`、`dashboard/activeRequests`、Dashboard 页面 |
| 按客户端 IP 并发排队 | 防止单个内网 IP 把网关并发打满 | `gateway/routing/request_gate.rs`、`gateway/upstream/proxy.rs` |
| CodexManagerLocal 本地发布工作流 | 用独立产品名和 bundle identifier 打包本地版 | `.github/workflows/release-local.yml`、`apps/src-tauri/tauri.local.conf.json`、`assets/macos-local/` |

## 1. 客户端 IP 识别与内网 IP 用量统计

### 目的

记录请求真实来源 IP，把 `client_ip` 写入请求日志和 Token 统计。前端可以在日志页看到客户端 IP，也可以在 API Keys 页面查看“内网 IP 用量”和“今日 Token”。

### 实现链路

1. 新增可信 IP 解析工具：
   - `crates/service/src/client_ip.rs`
   - 定义 `x-codexmanager-client-ip` 专用 header。
   - `set_forwarded_client_ip_header()` 由本地前置代理写入真实 peer IP。
   - `resolve_trusted_client_ip()` 只有在 service 入口远端是 loopback 时才信任该 header，避免外部客户端伪造 IP。

2. 前置代理注入 IP：
   - `crates/service/src/http/proxy_runtime.rs` 从 `ConnectInfo(peer_addr)` 取连接来源并注入 header。
   - `crates/web/src/service_gateway.rs` 过滤外部传入的同名 header，再写入 Web gateway 看到的 peer IP。

3. gateway 主链路传递 IP：
   - `crates/service/src/gateway/request/request_entry.rs` 在 `handle_gateway_request()` 开始处解析 `client_ip`。
   - `crates/service/src/gateway/local_validation/mod.rs` 和 `request.rs` 把 `client_ip` 放进 `LocalValidationResult`。
   - 本地 models、count_tokens、upstream proxy、错误日志等路径都继续传递该字段。

4. 请求日志和 Token 统计落库：
   - `crates/service/src/gateway/observability/request_log.rs` 在 `RequestLogTraceContext` 中加入 `client_ip`。
   - `write_request_log()` 同时把 `client_ip` 写入 `RequestLog` 和 `RequestTokenStat`。
   - `crates/core/src/storage/request_logs.rs` 为 `request_logs` 加 `client_ip` 字段和索引。
   - `crates/core/src/storage/request_token_stats.rs` 为 `request_token_stats` 加 `client_ip` 字段、索引、历史回填逻辑。

5. 按 IP 聚合 Token：
   - `request_token_stats.rs` 新增 `request_token_stat_client_ip_hourly_rollups` 表。
   - `summarize_request_token_stats_by_key_and_client_ip_between()` 合并 raw stats 与 hourly rollups，按 `key_id + client_ip` 汇总请求数、成功数、异常数、Token、费用、最近出现时间。
   - 迁移时要保留 raw 与 hourly 两路聚合，否则清理 raw logs 后 IP 用量会丢。

6. RPC 与权限：
   - `crates/core/src/rpc/types.rs` 新增 `ClientIpUsageListParams`、`ClientIpUsageSummaryResult`、`ClientIpUsageListResult`。
   - `crates/service/src/requestlog/requestlog_client_ip_usage.rs` 读取聚合数据，默认范围 30 天，默认 limit 100，最大 500。
   - `crates/service/src/rpc_dispatch/requestlog.rs` 暴露 `requestlog/client_ip_usage`。
   - 管理员可看全部；普通成员通过 `member_requestlog_scope()` 限制到自己拥有的 API Key。

7. 桌面/Web 前端入口：
   - `apps/src-tauri/src/commands/requestlog.rs` 新增 `service_requestlog_client_ip_usage`。
   - `apps/src-tauri/src/commands/registry.rs` 注册 Tauri 命令。
   - `apps/src/lib/api/transport-web-commands/misc.ts` 增加 Web command 映射。
   - `apps/src/lib/api/service-client.ts` 增加 `listClientIpUsage()`。
   - `apps/src/lib/api/normalize.ts` 增加 `normalizeClientIpUsageListResult()`。
   - `apps/src/types/request-log.ts` 增加 `clientIp` 和 `ClientIpUsageSummary` 类型。

8. UI 展示：
   - `apps/src/app/logs/page-cells.tsx` 新增 `ClientIpCell`。
   - `apps/src/app/logs/page-sections.tsx` 日志表增加“客户端 IP”列，搜索提示支持 IP。
   - `apps/src/app/apikeys/page.tsx` 增加“内网 IP 用量”表。
   - `5a38f09f` 在 API Keys 页补了一条当天范围查询：先按默认范围拿累计 IP 用量，再用本地当天 `startTs/endTs` 查询今日 IP Token，用 `${keyId}:${clientIp}` 合并展示。

### 迁移文件清单

优先迁移这些文件或对应 diff：

- `crates/service/src/client_ip.rs`
- `crates/service/src/lib.rs`
- `crates/service/src/http/proxy_runtime.rs`
- `crates/web/src/service_gateway.rs`
- `crates/service/src/gateway/request/request_entry.rs`
- `crates/service/src/gateway/local_validation/mod.rs`
- `crates/service/src/gateway/local_validation/request.rs`
- `crates/service/src/gateway/observability/request_log.rs`
- `crates/core/src/storage/request_logs.rs`
- `crates/core/src/storage/request_token_stats.rs`
- `crates/core/src/rpc/types.rs`
- `crates/service/src/requestlog/mod.rs`
- `crates/service/src/requestlog/requestlog_client_ip_usage.rs`
- `crates/service/src/rpc_dispatch/requestlog.rs`
- `apps/src-tauri/src/commands/requestlog.rs`
- `apps/src-tauri/src/commands/registry.rs`
- `apps/src/lib/api/service-client.ts`
- `apps/src/lib/api/normalize.ts`
- `apps/src/lib/api/transport-web-commands/misc.ts`
- `apps/src/types/request-log.ts`
- `apps/src/app/logs/page-cells.tsx`
- `apps/src/app/logs/page-sections.tsx`
- `apps/src/app/apikeys/page.tsx`

### 相关测试

- `apps/tests/request-log-client-ip-normalize.test.mjs`
- `apps/tests/request-log-client-ip-ui.test.mjs`
- `apps/tests/apikey-client-ip-usage.test.mjs`
- `crates/service/src/requestlog/requestlog_client_ip_usage_tests.rs`
- `crates/core/src/storage/tests/request_logs_tests.rs`
- `crates/core/src/storage/tests/request_token_stats_tests.rs`
- `crates/web/src/service_gateway_tests.rs`
- `crates/service/src/http/tests/proxy_runtime_tests.rs`
- `crates/service/src/http/responses_websocket_tests.rs`

## 2. Dashboard 实时活跃请求监控

### 目的

管理员 Dashboard 显示当前 gateway 内正在运行和排队的请求，包含状态、客户端 IP、API Key、路径、模型、路由来源、等待时间和运行时长。

### 实现链路

1. gateway 内存态活动表：
   - `crates/service/src/gateway/observability/request_activity.rs`
   - 使用全局 `OnceLock<Mutex<HashMap<String, RequestActivityEntry>>>`，key 是 `trace_id`。
   - `begin_request_activity()` 创建 entry，默认 `queued`。
   - `RequestActivityGuard` 在 `Drop` 时删除 entry，避免请求结束后残留。
   - `mark_request_activity_queued()`、`mark_request_activity_running()` 更新状态。
   - `update_request_activity_source()` 在选定实际来源后写入 `source_kind/source_id`。
   - `request_activity_snapshot(limit)` 生成 Dashboard RPC 结果，running 优先，其次 queued，再按创建顺序排序。

2. gateway 插桩：
   - `crates/service/src/gateway/upstream/proxy.rs` 在 upstream proxy 入口调用 `begin_request_activity()`。
   - 进入 IP gate 或原有 request gate 时标记 queued/running。
   - 候选执行器命中实际来源后更新 source 信息。

3. Dashboard RPC：
   - `crates/service/src/dashboard.rs` 新增 `read_active_requests()`。
   - 只允许管理员读取；普通成员返回 `permission_denied`。
   - limit 默认 50，并 clamp 到 1 到 50。
   - `crates/service/src/rpc_dispatch/dashboard.rs` 暴露 `dashboard/activeRequests`。
   - `crates/core/src/rpc/types.rs` 增加 `DashboardActiveRequestItem`、`DashboardActiveRequestsResult`。

4. 桌面前端调用：
   - `apps/src-tauri/src/commands/dashboard.rs` 新增 `service_dashboard_active_requests`。
   - `apps/src-tauri/src/commands/registry.rs` 注册 Tauri 命令。
   - `apps/src/lib/api/dashboard-client.ts` 增加 `getActiveRequests()` 并做 camel/snake 字段兼容读取。
   - `apps/src/types/dashboard.ts` 增加前端类型。
   - `apps/src/hooks/useDashboardActiveRequests.ts` 用 TanStack Query 每 1500ms 轮询。

5. UI 展示：
   - `apps/src/app/page.tsx` 增加 `AdminActiveRequestsCard`。
   - 只在管理员 Dashboard 挂载，不在成员 Dashboard 挂载。
   - hook 只在 desktop runtime、页面激活、service connected、admin 下启用。
   - 支持全部/运行中/排队中筛选。

注意：当前 Web command map 没有看到 `service_dashboard_active_requests` 映射，前端 hook 也明确限制 `isDesktopRuntime`。如果新版项目希望 Web UI 也显示实时请求，需要额外在 `apps/src/lib/api/transport-web-commands/` 增加映射，并评估权限暴露。

### 迁移文件清单

- `crates/service/src/gateway/observability/request_activity.rs`
- `crates/service/src/gateway/mod.rs`
- `crates/service/src/gateway/upstream/proxy.rs`
- `crates/service/src/gateway/upstream/proxy_pipeline/candidate_executor.rs`
- `crates/service/src/gateway/upstream/proxy_pipeline/request_gate.rs`
- `crates/service/src/dashboard.rs`
- `crates/service/src/rpc_dispatch/dashboard.rs`
- `crates/core/src/rpc/types.rs`
- `apps/src-tauri/src/commands/dashboard.rs`
- `apps/src-tauri/src/commands/registry.rs`
- `apps/src/lib/api/dashboard-client.ts`
- `apps/src/types/dashboard.ts`
- `apps/src/hooks/useDashboardActiveRequests.ts`
- `apps/src/app/page.tsx`

### 相关测试

- `apps/tests/dashboard-active-requests.test.mjs`
- `crates/service/src/dashboard_tests.rs`
- `crates/service/src/gateway/observability/request_activity.rs` 内联测试
- `apps/tests/tauri-command-registry.test.mjs`

## 3. 按客户端 IP 的并发排队

### 目的

限制单个客户端 IP 的并发请求数，避免一台内网机器把 gateway 或上游账号池打满。当前实现是同一个 IP 最多 4 个 running 请求，不同 IP 互相隔离。

### 实现链路

1. 扩展 request gate：
   - `crates/service/src/gateway/routing/request_gate.rs`
   - 原有 `request_gate_lock(key_id, path, model)` 保持单 scope 同时 1 个 running。
   - 新增 `CLIENT_IP_GATE_MAX_RUNNING: usize = 4`。
   - 新增 `client_ip_gate_lock(client_ip)`，key 形如 `client_ip|192.168.1.20`。
   - 复用同一张内存 lock table 和 TTL 清理逻辑。
   - `RequestGateLock::with_max_running()` 支持非 1 的并发上限。

2. upstream proxy 加前置 gate：
   - `crates/service/src/gateway/upstream/proxy.rs`
   - `acquire_client_ip_request_gate()` 在主要候选执行前调用。
   - 没有 `client_ip` 时跳过 IP gate，但仍把活动请求标记为 running。
   - 同 IP 已满时先标记为 `client_ip_gate` queued，再等待。
   - 等待成功后标记 running。
   - 等待超时返回 504 `client IP request queue wait timeout`。
   - lock poisoned 返回 503 `client IP request gate unavailable`。
   - guard 通过 RAII 在请求结束时释放并通知下一个等待者。

3. 与实时 Dashboard 联动：
   - 等待 IP gate 时，Dashboard 会显示 queued。
   - 获得 gate 后，Dashboard 会显示 running。
   - UI 上能看到同 IP 请求是否堆积。

### 迁移文件清单

- `crates/service/src/gateway/routing/request_gate.rs`
- `crates/service/src/gateway/upstream/proxy.rs`
- `crates/service/src/gateway/mod.rs`
- `apps/src/app/page.tsx` 中实时请求状态筛选相关片段

### 相关测试

- `crates/service/src/gateway/routing/tests/request_gate_tests.rs`
- `apps/tests/dashboard-active-requests.test.mjs`

迁移时可以考虑把 `CLIENT_IP_GATE_MAX_RUNNING = 4` 改成配置项；当前本地实现是硬编码常量。

## 4. CodexManagerLocal 本地发布工作流

### 目的

用独立产品名 `CodexManagerLocal` 构建本地分叉，避免和作者原版桌面应用冲突，并提供 Windows NSIS 与 macOS x64 DMG 发布产物。

### 实现链路

1. 本地 Tauri 配置：
   - `apps/src-tauri/tauri.local.conf.json`
   - 覆盖 `productName: CodexManagerLocal`
   - 覆盖 `identifier: com.codexmanager.local`
   - 覆盖窗口标题 `CodexManager Local`

2. GitHub Actions workflow：
   - `.github/workflows/release-local.yml`
   - `workflow_dispatch` 参数：`tag`、`ref`、`prerelease`。
   - `build_frontend_dist` 先在 Ubuntu 构建 `apps/out` 并上传 artifact。
   - Windows job 使用 `pnpm dlx @tauri-apps/cli@2.10.1 build --bundles nsis --config src-tauri/tauri.local.conf.json --ci`。
   - macOS job 使用 `--bundles app --target x86_64-apple-darwin --config src-tauri/tauri.local.conf.json --ci`。
   - macOS job 手动重建 DMG，把 `CodexManagerLocal.app`、Applications symlink、首启脚本、说明文件放入 DMG。
   - release job 下载 Windows/macOS artifacts 后发布 GitHub Release，支持覆盖同 tag assets。

3. macOS 首启辅助：
   - `assets/macos-local/Open CodexManagerLocal.command`
   - 自动定位 `/Applications/CodexManagerLocal.app` 或 DMG 同目录 app。
   - 执行 `xattr -dr com.apple.quarantine` 后打开应用。
   - `assets/macos-local/README-macOS-first-launch-local.txt` 说明未 notarize 时的打开方式。

4. 配套测试：
   - `apps/tests/local-release-workflow.test.mjs`
   - 校验 local Tauri config 存在、产品名/identifier/窗口标题正确。
   - 校验 workflow 使用正确的 config 相对路径和本地版产物名称。

### 迁移文件清单

- `.github/workflows/release-local.yml`
- `apps/src-tauri/tauri.local.conf.json`
- `assets/macos-local/Open CodexManagerLocal.command`
- `assets/macos-local/README-macOS-first-launch-local.txt`
- `apps/tests/local-release-workflow.test.mjs`
- `apps/package.json` 中 `test:runtime` 的测试列表变更

## 5. 可选支持项

这些不是核心功能，但迁移时可以按需要保留：

- `.cargo/config.toml`：把 crates.io 替换为 `rsproxy.cn`，并关闭 HTTP multiplexing。适合国内网络环境，但如果新版项目/CI 不需要，可以不迁移。
- `.gitignore` 和 `apps/.gitignore`：忽略本地 agent、构建或运行产物，属于本地开发卫生项。

## 建议迁移顺序

1. 在作者新版项目上新建迁移分支。
2. 先迁移 core 存储字段、RPC types 和 service 端 IP 传递，保证 Rust 编译通过。
3. 迁移 `requestlog/client_ip_usage` RPC 和前端 API/类型/normalize，再接 API Keys 与 Logs 页面。
4. 迁移 `request_activity` 和 Dashboard active requests，确认 desktop 命令注册完整。
5. 迁移 `client_ip_gate` 并确认 gateway deadline、trace log、activity 状态没有被新版 upstream 改坏。
6. 最后迁移 `release-local` workflow 和 `CodexManagerLocal` 配置，因为它与业务运行逻辑相对独立。

## 建议验证命令

根据迁移范围选择最小验证：

```powershell
cargo test -p codexmanager-core
cargo test -p codexmanager-service
cargo test -p codexmanager-web
pnpm -C apps run test:runtime
pnpm -C apps run build:desktop
```

如果只迁移文档或 workflow，可至少运行：

```powershell
git diff --check
```

## 迁移风险点

- IP header 安全边界不能漏：外部请求不能直接信任 `x-codexmanager-client-ip`，只能信任本地 proxy/web gateway 写入并从 loopback 到达 service 的值。
- `client_ip` 要同时进入 `request_logs` 和 `request_token_stats`，否则日志页能看到 IP 但 Token 统计无法按 IP 聚合。
- hourly rollup 不能只迁移普通 Token 汇总表，要保留 `request_token_stat_client_ip_hourly_rollups`，否则 raw stats 被清理后 IP 用量会消失。
- Dashboard active requests 是内存快照，不是数据库日志查询；迁移时不要改成查 request logs。
- `dashboard/activeRequests` 目前是管理员和 desktop runtime 限定能力；如果开放 Web，需要重新检查权限和 Web command 映射。
- `CLIENT_IP_GATE_MAX_RUNNING` 当前硬编码为 4；如果新版项目已有并发配置系统，建议改成配置项，但先保持行为一致更稳。
