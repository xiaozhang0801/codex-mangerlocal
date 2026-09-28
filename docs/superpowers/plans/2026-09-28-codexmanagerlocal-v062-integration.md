# CodexManagerLocal v0.6.2 Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 以作者 `origin/main` 的 v0.6.2 异步桌面架构为基础，恢复并适配 ZhangHao 的本地 App 功能，同时保留作者新增能力、产品身份隔离和现有页面布局。

**Architecture:** 先从 `origin/main` 建立 v0.6.2 集成分支，再把本地功能按新架构重新接入，不直接把旧版文件覆盖到作者新版。请求日志和 Token 统计通过 `codexmanager-storage-seaorm` 的运行时存储边界持久化，可信客户端 IP 沿 Axum middleware、`GatewayRequest`、gateway observability 和 RPC 传递；实时请求使用内存 RAII tracker，IP 并发使用独立的动态客户端 IP gate。桌面 App 只增加必要的数据/API/状态展示，不重做样式、布局或页面操作流程。

**Tech Stack:** Rust 2021、Tokio、Axum 0.8、SeaORM、SQLite、Tauri v2、Next.js 16 静态导出、React 19、TanStack Query、Node test runner、GitHub Actions。

**Spec:** `docs/superpowers/specs/2026-09-28-codexmanagerlocal-v062-integration-design.md`

## Global Constraints

- 目标产品名必须是 `CodexManagerLocal`。
- Tauri identifier 必须是 `com.codexmanager.local`。
- 更新检查、Release 下载和更新资产必须指向 `xiaozhang0801/codex-mangerlocal`。
- IP 用量按客户端 IP 汇总，同一个 IP 使用多个 API Key 时只显示一行。
- IP 用量必须保留 Token、估算金额、请求数、成功/异常数、最近请求时间，并支持排序。
- 只有一个活跃客户端 IP 时不限制该 IP 的 4 并发；活跃 IP 数大于 1 时，每个 IP 最多 4 个运行中请求。
- 没有可信客户端 IP 时跳过 IP gate，但继续执行作者已有的其他 gate。
- 实时请求仅在桌面 App 的管理员 Dashboard 展示和轮询；不开发 Web UI 的本地实时功能。
- 不删除作者 v0.6.2 的账号自动唤醒、批量开关、异步 runtime、HTTP 超时、panic 防护、health、metrics、SeaORM、模型目录/价格同步、Codex 直接聚合 API 及其修复。
- 不重做页面视觉样式、整体布局或已有操作流程。
- 所有新增行为先写失败测试，再写最小实现；每个任务结束后运行其定点验证。
- 不直接把作者的 `CodexManager`、`com.codexmanager.desktop` 或 `qxcnm/Codex-Manager` 配置带入本地版。
- 不开发或验证 `crates/web` 的本地 IP 用量页面；只在作者编译链要求时保持 Web crate 可编译。

## Review Focus

- 外部客户端伪造 `x-codexmanager-client-ip` 时，服务必须使用真实连接 peer 或空值，而不能相信伪造值；由 Task 3 的可信 IP 测试覆盖。
- 同一 IP 跨多个 API Key、账号和模型时，累计 Token、金额、请求数和今日 Token 必须合并为一行，且空 IP 不得生成展示行；由 Task 4 和 Task 5 的聚合测试覆盖。
- 下游断开、上游错误、超时和 panic 后，实时请求条目和 IP 并发名额必须清理；由 Task 6 和 Task 7 的 cancellation/guard 测试覆盖。
- 单 IP 已经有超过 4 个运行请求后，第二个 IP 出现时只限制后续请求；第二个 IP 退出后，单 IP 状态必须恢复放行；由 Task 7 的状态转移测试覆盖。
- 作者更新后的默认身份、更新仓库、数据库路径和发布资产名称不能悄悄回退到作者版；由 Task 2 和 Task 8 的静态配置回归测试覆盖。

## File Structure

### Author v0.6.2 baseline and local identity

- Modify: `Cargo.toml`, `Cargo.lock`, `apps/package.json`, `apps/src-tauri/Cargo.toml`, `apps/src-tauri/Cargo.lock`: retain the upstream v0.6.2 dependency graph while restoring the local package identity.
- Modify: `apps/src-tauri/tauri.conf.json`, `apps/src-tauri/src/app_storage/migration.rs`: restore local Tauri identity and verify the database/installation namespace follows the local identifier.
- Modify: `apps/src/app/layout.tsx`, `apps/src/components/layout/automatic-update-checker.tsx`, `apps/src/app/settings/settings-page-helpers.ts`: restore local product/release display and updater fallback.
- Modify: `apps/src-tauri/src/commands/updater/runtime.rs`, `apps/src-tauri/src/commands/updater/github.rs`: make updater metadata and asset resolution default to the local repository.
- Create: `.github/workflows/release-local.yml`, `assets/macos-local/Open CodexManagerLocal.command`, `assets/macos-local/README-macOS-first-launch-local.txt`.
- Create: `apps/tests/local-release-workflow.test.mjs`.

### Trusted client IP and request lifecycle

- Create: `crates/service/src/client_ip.rs`: parse and validate the internal header and resolve a client IP from a trusted peer.
- Modify: `crates/service/src/lib.rs`, `crates/service/src/http/middleware.rs`, `crates/service/src/http/router.rs`: expose the helper and sanitize/inject the internal IP header at the Axum boundary.
- Modify: `crates/service/src/http/gateway_request.rs`, `crates/service/src/gateway/request/request_entry.rs`, `crates/service/src/gateway/local_validation/mod.rs`, `crates/service/src/gateway/local_validation/request.rs`: carry `client_ip` into the validated request and native async gateway path.
- Modify: `crates/service/src/gateway/observability/request_log.rs`, `crates/service/src/gateway/upstream/proxy_pipeline/request_setup.rs`, `crates/service/src/gateway/upstream/proxy_pipeline/response_finalize.rs`: keep the IP in success, error, cancellation and usage-log paths.
- Create: `crates/service/tests/client_ip_tests.rs` or move equivalent coverage into the current service test module.

### Storage and IP usage aggregation

- Modify: `crates/core/src/storage/mod.rs`: add `client_ip` to shared `RequestLog` and `RequestTokenStat` records used by both storage backends.
- Modify: `crates/storage-seaorm/src/request_logs.rs`, `crates/storage-seaorm/src/request_token_stats.rs`, `crates/storage-seaorm/src/request_log_retention.rs`, `crates/storage-seaorm/src/migration.rs`: persist IP, preserve it during append/import/retention, and add indexed IP aggregation.
- Modify: `crates/core/src/rpc/types.rs`: define the IP usage and request activity DTOs with camelCase serialization.
- Test: `crates/storage-seaorm/src/request_logs.rs`, `crates/storage-seaorm/src/request_token_stats.rs`, `crates/storage-seaorm/src/real_database_tests.rs`, plus focused service storage tests.

### Desktop RPC and App UI

- Create: `crates/service/src/requestlog/client_ip_usage.rs` or extend `crates/service/src/requestlog/seaorm.rs`: read IP-aggregated usage after actor/key permission filtering.
- Modify: `crates/service/src/requestlog/mod.rs`, `crates/service/src/rpc_dispatch/requestlog.rs`: expose `requestlog/client_ip_usage`.
- Modify: `apps/src-tauri/src/commands/requestlog.rs`, `apps/src-tauri/src/commands/registry.rs`: add/register `service_requestlog_client_ip_usage`.
- Modify: `apps/src/lib/api/service-client.ts`, `apps/src/lib/api/normalize.ts`, `apps/src/types/request-log.ts`, `apps/src/types/index.ts`: add typed IP usage data and snake_case/camelCase normalization.
- Modify: `apps/src/app/logs/page-cells.tsx`, `apps/src/app/logs/page-sections.tsx`: show/search client IP in the existing logs table.
- Modify: `apps/src/app/apikeys/page.tsx`: render one IP row with cumulative/today Token and amount data, plus sorting controls using the existing UI primitives.
- Create: `apps/tests/request-log-client-ip-normalize.test.mjs`, `apps/tests/request-log-client-ip-ui.test.mjs`, `apps/tests/apikey-client-ip-usage.test.mjs`.
- Do not modify `apps/src/lib/api/transport-web-commands/` for this local feature; desktop App is the requested surface.

### Realtime active requests and conditional IP gate

- Create: `crates/service/src/gateway/observability/request_activity.rs`: in-memory `RequestActivityGuard`, queued/running state and full-IP snapshot aggregation.
- Modify: `crates/service/src/gateway/mod.rs`, `crates/service/src/http/gateway_endpoint.rs`, `crates/service/src/http/gateway_request.rs`, `crates/service/src/gateway/request/request_entry.rs`: create/clear activity around the async request lifetime.
- Modify: `crates/service/src/gateway/upstream/proxy_pipeline/candidate_executor.rs`: record the selected route/source without changing author retry semantics.
- Modify: `crates/service/src/gateway/routing/request_gate.rs`, `crates/service/src/gateway/upstream/proxy_pipeline/request_gate.rs`: add dynamic per-IP state while preserving the author's async fixed gate and cancellation behavior.
- Modify: `crates/service/src/dashboard.rs`, `crates/service/src/rpc_dispatch/dashboard.rs`: add admin-only `dashboard/activeRequests`.
- Modify: `apps/src-tauri/src/commands/dashboard.rs`, `apps/src-tauri/src/commands/registry.rs`, `apps/src/lib/api/dashboard-client.ts`, `apps/src/types/dashboard.ts`, `apps/src/types/index.ts`: add desktop RPC and normalized types.
- Create: `apps/src/hooks/useDashboardActiveRequests.ts`.
- Modify: `apps/src/app/page.tsx`: mount the existing-style realtime panel only in the admin Dashboard, gated by desktop runtime, page visibility, service readiness and admin session.
- Create: `apps/tests/dashboard-active-requests.test.mjs`.
- Test: `crates/service/src/gateway/observability/request_activity.rs`, `crates/service/src/gateway/routing/tests/request_gate_tests.rs`, `crates/service/src/dashboard_tests.rs`.

---

### Task 1: Establish The v0.6.2 Integration Baseline

**Files:**
- Create: a temporary implementation branch from `origin/main`; preserve the current `main` branch unchanged until final validation.
- Modify: `docs/superpowers/specs/2026-09-28-codexmanagerlocal-v062-integration-design.md` only if a source path has materially changed; do not alter product requirements.
- Verify: `Cargo.toml`, `crates/storage-seaorm/`, `crates/service/src/http/`, `apps/src-tauri/`, `apps/src/app/`.

**Interfaces:**
- Consumes: `origin/main` at `09ba93db` / tag `v0.6.2`.
- Produces: an integration branch whose default service path is Axum + async runtime + SeaORM, with the author's new tests/builds available before local behavior is reintroduced.

- [ ] **Step 1: Create the isolated integration branch**

Run:

```powershell
git fetch origin main --tags
git switch -c codexmanagerlocal-v062-integration origin/main
git restore --source=46cec4bb -- docs/superpowers/specs/2026-09-28-codexmanagerlocal-v062-integration-design.md
```

Expected: the branch is based on `09ba93db`, the design document is staged as a new file for the baseline commit, and the original `main` ref is untouched.

- [ ] **Step 2: Record the actual v0.6.2 module boundaries**

Run:

```powershell
git ls-tree -r --name-only HEAD crates/service/src/http crates/service/src/gateway crates/storage-seaorm/src
git grep -n "service_dashboard_active_requests\|service_requestlog_client_ip_usage\|client_ip" -- apps crates
```

Expected: the report identifies removed local modules and their new async/storage replacement points before any port is attempted.

- [ ] **Step 3: Run the author's narrow baseline tests**

Run:

```powershell
cargo test -p codexmanager-service --lib -- --test-threads=1
cargo test -p codexmanager-storage-seaorm --lib -- --test-threads=1
pnpm -C apps run test:runtime
pnpm -C apps run build:desktop
```

Expected: the author baseline passes, or every pre-existing failure is recorded with its exact command and output before local changes begin.

- [ ] **Step 4: Commit the baseline marker**

Run:

```powershell
git add -- docs/superpowers/specs/2026-09-28-codexmanagerlocal-v062-integration-design.md
git commit -m "chore: 建立 CodexManagerLocal v0.6.2 集成基线"
```

Expected: only the design document is committed at this step.

---

### Task 2: Restore Local Identity, Database Isolation, Updater And Release

**Files:**
- Modify: `apps/src-tauri/tauri.conf.json`, `apps/src-tauri/Cargo.toml`, `apps/src-tauri/src/app_storage/migration.rs`.
- Modify: `apps/src/app/layout.tsx`, `apps/src/components/layout/automatic-update-checker.tsx`, `apps/src/app/settings/settings-page-helpers.ts`.
- Modify: `apps/src-tauri/src/commands/updater/runtime.rs`, `apps/src-tauri/src/commands/updater/github.rs`.
- Create: `.github/workflows/release-local.yml`, `apps/src-tauri/tauri.local.conf.json`, `assets/macos-local/Open CodexManagerLocal.command`, `assets/macos-local/README-macOS-first-launch-local.txt`.
- Create: `apps/tests/local-release-workflow.test.mjs`.

**Interfaces:**
- Produces: `productName = "CodexManagerLocal"`, `identifier = "com.codexmanager.local"`, window title `CodexManager Local`, and updater repository `xiaozhang0801/codex-mangerlocal`.
- Consumes: author `release-all.yml`, reusable release actions, current updater command contract and Tauri config merge rules.

- [ ] **Step 1: Write the failing identity/release test**

The test must assert all of the following:

```js
assert.equal(config.productName, "CodexManagerLocal");
assert.equal(config.identifier, "com.codexmanager.local");
assert.equal(config.app.windows[0].title, "CodexManager Local");
assert.match(updaterSource, /xiaozhang0801\/codex-mangerlocal/);
assert.match(workflowSource, /CodexManagerLocal/);
assert.match(workflowSource, /x64-setup\.exe/);
assert.match(workflowSource, /x64\.dmg/);
assert.doesNotMatch(updaterSource, /qxcnm\/Codex-Manager/);
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```powershell
pnpm -C apps exec node --test tests/local-release-workflow.test.mjs
```

Expected: FAIL because the upstream config and updater still identify the author product and the local workflow is absent.

- [ ] **Step 3: Restore the local Tauri identity**

Set the exact values:

```text
productName: CodexManagerLocal
identifier: com.codexmanager.local
window title: CodexManager Local
```

Update the package name only where Tauri packaging/database identity depends on it. Keep upstream version `0.6.2` and all author runtime dependencies.

- [ ] **Step 4: Restore updater defaults**

Make `buildReleaseUrl()`, automatic update fallback, GitHub asset lookup and updater runtime defaults resolve:

```text
https://github.com/xiaozhang0801/codex-mangerlocal/releases
```

The implementation must still honor an explicit configured release repository when the existing settings contract provides one; only the default may change.

- [ ] **Step 5: Add the local release workflow**

Create `.github/workflows/release-local.yml` by adapting the author's current workflow/actions. It must:

- build the frontend and desktop App from the selected `ref`;
- build Windows NSIS and macOS x64 App/DMG;
- use `apps/src-tauri/tauri.local.conf.json`;
- publish to the current repository rather than hard-coding the author's repository;
- stage `CodexManagerLocal_${version}_x64-setup.exe` and `CodexManagerLocal_${version}_x64.dmg`;
- preserve the macOS first-launch helper assets.

- [ ] **Step 6: Rerun the focused test**

Run:

```powershell
pnpm -C apps exec node --test tests/local-release-workflow.test.mjs
```

Expected: PASS, with no author repository or product identity in the local workflow/default updater path.

- [ ] **Step 7: Commit the identity and release slice**

Run:

```powershell
git add -- apps/src-tauri/tauri.conf.json apps/src-tauri/Cargo.toml apps/src-tauri/src/app_storage/migration.rs apps/src/app/layout.tsx apps/src/components/layout/automatic-update-checker.tsx apps/src/app/settings/settings-page-helpers.ts apps/src-tauri/src/commands/updater/runtime.rs apps/src-tauri/src/commands/updater/github.rs apps/src-tauri/tauri.local.conf.json .github/workflows/release-local.yml assets/macos-local apps/tests/local-release-workflow.test.mjs
git commit -m "feat(release): 恢复 CodexManagerLocal 身份与更新地址"
```

---

### Task 3: Reconnect Trusted Client IP To The Async Gateway

**Files:**
- Create: `crates/service/src/client_ip.rs`.
- Modify: `crates/service/src/lib.rs`, `crates/service/src/http/middleware.rs`, `crates/service/src/http/router.rs`.
- Modify: `crates/service/src/http/gateway_request.rs`, `crates/service/src/gateway/request/request_entry.rs`.
- Modify: `crates/service/src/gateway/local_validation/mod.rs`, `crates/service/src/gateway/local_validation/request.rs`.
- Test: `crates/service/tests/client_ip_tests.rs`, `crates/service/src/http/tests/middleware_policy_tests.rs`.

**Interfaces:**
- `pub const FORWARDED_CLIENT_IP_HEADER: &str = "x-codexmanager-client-ip";`
- `pub fn resolve_trusted_client_ip(remote_addr: Option<&SocketAddr>, forwarded: Option<&str>) -> Option<String>;`
- `pub fn set_forwarded_client_ip_header(headers: &mut HeaderMap, peer_addr: SocketAddr);`
- `GatewayRequest` exposes its original peer address and sanitized headers to the request entry.

- [ ] **Step 1: Write failing trust-boundary tests**

Cover:

```rust
assert_eq!(
    resolve_trusted_client_ip(Some(&"127.0.0.1:4000".parse().unwrap()), Some("192.168.1.20")),
    Some("192.168.1.20".into())
);
assert_eq!(
    resolve_trusted_client_ip(Some(&"10.0.0.10:4000".parse().unwrap()), Some("192.168.1.20")),
    Some("10.0.0.10".into())
);
assert_eq!(
    resolve_trusted_client_ip(Some(&"127.0.0.1:4000".parse().unwrap()), Some("not-an-ip")),
    Some("127.0.0.1".into())
);
```

Also assert that middleware removes an external copy before inserting the observed peer value.

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```powershell
cargo test -p codexmanager-service client_ip -- --test-threads=1
```

Expected: FAIL because the helper and new gateway propagation are absent.

- [ ] **Step 3: Implement the helper and Axum boundary**

Use `std::net::IpAddr` parsing. Accept the internal header only when the direct service peer is loopback. Do not infer `127.0.0.1`, LAN IP, or a forwarded chain when no valid peer exists.

Apply the middleware at the service router boundary so externally supplied `x-codexmanager-client-ip` is removed and a peer-derived value is inserted only for the internal forwarding path.

- [ ] **Step 4: Carry IP through `GatewayRequest` and local validation**

Resolve the value once at `handle_gateway_request_async()`. Add `client_ip: Option<String>` to the validation result and pass `Option<&str>` through local models, count-tokens, response, proxy setup, and terminal error logging.

- [ ] **Step 5: Rerun focused tests**

Run:

```powershell
cargo test -p codexmanager-service client_ip -- --test-threads=1
cargo test -p codexmanager-service middleware_policy -- --test-threads=1
```

Expected: PASS, including the non-loopback spoofing case.

- [ ] **Step 6: Commit trusted IP plumbing**

Run:

```powershell
git add -- crates/service/src/client_ip.rs crates/service/src/lib.rs crates/service/src/http/middleware.rs crates/service/src/http/router.rs crates/service/src/http/gateway_request.rs crates/service/src/gateway/request/request_entry.rs crates/service/src/gateway/local_validation/mod.rs crates/service/src/gateway/local_validation/request.rs crates/service/tests/client_ip_tests.rs crates/service/src/http/tests/middleware_policy_tests.rs
git commit -m "feat(gateway): 接入异步服务的可信客户端 IP"
```

---

### Task 4: Persist And Aggregate IP Usage In The v0.6.2 Storage Boundary

**Files:**
- Modify: `crates/core/src/storage/mod.rs`.
- Modify: `crates/storage-seaorm/src/request_logs.rs`, `crates/storage-seaorm/src/request_token_stats.rs`, `crates/storage-seaorm/src/request_log_retention.rs`, `crates/storage-seaorm/src/migration.rs`.
- Modify: the active service observability writer in `crates/service/src/gateway/observability/request_log.rs` and `crates/service/src/requestlog/seaorm.rs`.
- Test: storage repository tests and service request-log observability tests.

**Interfaces:**
- `RequestLog.client_ip: Option<String>`.
- `RequestTokenStat.client_ip: Option<String>`.
- `ClientIpUsageSummary` contains `client_ip`, request/success/error counts, input/cached/output/reasoning/total tokens, `estimated_cost_usd`, and `last_seen_at`.
- `RequestLogsRepository::append_with_usage()` stores the same IP on the request log and token stat.
- `RequestTokenStatsRepository::summarize_by_client_ip_between(start_ts, end_ts, key_ids)` returns one result per non-empty IP ordered by `total_tokens DESC, client_ip ASC`.

- [ ] **Step 1: Write failing SeaORM aggregation tests**

Insert two request/token records for the same IP with different `key_id` values and assert:

```rust
assert_eq!(items.len(), 1);
assert_eq!(items[0].client_ip, "192.168.1.20");
assert_eq!(items[0].total_tokens, 300);
assert_eq!(items[0].estimated_cost_usd, 0.03);
assert_eq!(items[0].request_count, 2);
```

Add a permission-filter fixture where the same IP has one visible and one invisible key; the filtered query must include only the visible key before grouping.

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```powershell
cargo test -p codexmanager-storage-seaorm request_log -- --test-threads=1
cargo test -p codexmanager-storage-seaorm request_token -- --test-threads=1
```

Expected: FAIL because `client_ip` is not in the v0.6.2 SeaORM models and no IP aggregate exists.

- [ ] **Step 3: Add fields and migrations**

Add nullable `client_ip` to shared records and SeaORM models. Add the migration/index needed by the active backend. Existing rows may have null IP and must remain readable. Do not make historical IP backfill block startup; when a legacy request log has an IP, backfill its matching token stat transactionally.

- [ ] **Step 4: Implement raw-plus-rollup aggregation**

Keep raw records with `key_id + client_ip` for permission filtering and auditing. Aggregate only after key filtering. If retention uses hourly rollups, add IP to that rollup key and merge raw and rollup rows without double counting. Ignore blank/null IPs for the App display result.

- [ ] **Step 5: Write IP on every observability outcome**

Update normal response, model/count-token response, validation error, upstream error, timeout and cancellation paths so the same request's log and token stat carry the resolved IP.

- [ ] **Step 6: Rerun focused storage tests**

Run:

```powershell
cargo test -p codexmanager-storage-seaorm request_log -- --test-threads=1
cargo test -p codexmanager-storage-seaorm request_token -- --test-threads=1
cargo test -p codexmanager-service request_log --lib -- --test-threads=1
```

Expected: PASS, with one aggregated row for the same IP across multiple keys.

- [ ] **Step 7: Commit the storage slice**

Run:

```powershell
git add -- crates/core/src/storage/mod.rs crates/storage-seaorm/src/request_logs.rs crates/storage-seaorm/src/request_token_stats.rs crates/storage-seaorm/src/request_log_retention.rs crates/storage-seaorm/src/migration.rs crates/service/src/gateway/observability/request_log.rs crates/service/src/requestlog/seaorm.rs
git commit -m "feat(storage): 按客户端 IP 汇总 Token 与金额"
```

---

### Task 5: Expose IP Usage Through Desktop RPC And Existing App Surfaces

**Files:**
- Create or modify: `crates/service/src/requestlog/client_ip_usage.rs`, `crates/service/src/requestlog/mod.rs`, `crates/service/src/rpc_dispatch/requestlog.rs`.
- Modify: `crates/core/src/rpc/types.rs`.
- Modify: `apps/src-tauri/src/commands/requestlog.rs`, `apps/src-tauri/src/commands/registry.rs`.
- Modify: `apps/src/lib/api/service-client.ts`, `apps/src/lib/api/normalize.ts`, `apps/src/types/request-log.ts`, `apps/src/types/index.ts`.
- Modify: `apps/src/app/logs/page-cells.tsx`, `apps/src/app/logs/page-sections.tsx`, `apps/src/app/apikeys/page.tsx`.
- Create: `apps/tests/request-log-client-ip-normalize.test.mjs`, `apps/tests/request-log-client-ip-ui.test.mjs`, `apps/tests/apikey-client-ip-usage.test.mjs`.

**Interfaces:**
- RPC method: `requestlog/client_ip_usage`.
- Tauri command: `service_requestlog_client_ip_usage(addr, start_ts, end_ts, limit)`.
- Frontend method: `serviceClient.listClientIpUsage({ startTs, endTs, limit })`.
- Frontend row key: `clientIp`; never use `keyId` as the display/grouping key.

- [ ] **Step 1: Write failing RPC/frontend contract tests**

Assert that:

```js
assert.match(commandSource, /requestlog\/client_ip_usage/);
assert.match(clientSource, /service_requestlog_client_ip_usage/);
assert.match(normalizeSource, /clientIp/);
assert.match(apiKeysSource, /listClientIpUsage/);
assert.doesNotMatch(apiKeysSource, /keyId:\s*item\.keyId/);
assert.match(logsSource, /客户端 IP/);
```

Also add a normalization fixture containing both `client_ip` and `clientIp` input forms.

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```powershell
pnpm -C apps exec node --test tests/request-log-client-ip-normalize.test.mjs tests/request-log-client-ip-ui.test.mjs tests/apikey-client-ip-usage.test.mjs
```

Expected: FAIL because the v0.6.2 App no longer exposes the local IP command/types/page section.

- [ ] **Step 3: Add typed DTOs and actor-scoped RPC**

Parse `ClientIpUsageListParams` with normalized default range and max limit. Admins read all rows; members first resolve visible key IDs, then aggregate by IP. The response must not include a key-split display list.

- [ ] **Step 4: Add the Tauri command and typed client**

Call the existing `rpc_call_in_background()`/`invoke()` transport chain. Keep the Web command map unchanged because local functionality is desktop-only.

- [ ] **Step 5: Restore IP display and sorting in Logs/API Keys**

Keep the existing table/card layout and styles. Add only:

- a client IP log cell and searchable IP text;
- cumulative Token, today Token, cumulative estimated cost, today estimated cost, request count and last-seen fields;
- sort options for today Token, total Token, today/total cost, request count, last seen and IP ascending.

Merge today totals by `clientIp`, not `${keyId}:${clientIp}`.

- [ ] **Step 6: Rerun focused frontend tests**

Run:

```powershell
pnpm -C apps exec node --test tests/request-log-client-ip-normalize.test.mjs tests/request-log-client-ip-ui.test.mjs tests/apikey-client-ip-usage.test.mjs
```

Expected: PASS, and the source contract contains no key-based IP display grouping.

- [ ] **Step 7: Commit the App IP usage slice**

Run:

```powershell
git add -- crates/core/src/rpc/types.rs crates/service/src/requestlog crates/service/src/rpc_dispatch/requestlog.rs apps/src-tauri/src/commands/requestlog.rs apps/src-tauri/src/commands/registry.rs apps/src/lib/api/service-client.ts apps/src/lib/api/normalize.ts apps/src/types/request-log.ts apps/src/types/index.ts apps/src/app/logs/page-cells.tsx apps/src/app/logs/page-sections.tsx apps/src/app/apikeys/page.tsx apps/tests/request-log-client-ip-normalize.test.mjs apps/tests/request-log-client-ip-ui.test.mjs apps/tests/apikey-client-ip-usage.test.mjs
git commit -m "feat(app): 恢复按内网 IP 汇总用量"
```

---

### Task 6: Add Realtime Active Request Tracking And Admin RPC

**Files:**
- Create: `crates/service/src/gateway/observability/request_activity.rs`.
- Modify: `crates/service/src/gateway/mod.rs`, `crates/service/src/http/gateway_endpoint.rs`, `crates/service/src/http/gateway_request.rs`, `crates/service/src/gateway/request/request_entry.rs`, `crates/service/src/gateway/upstream/proxy_pipeline/candidate_executor.rs`.
- Modify: `crates/core/src/rpc/types.rs`, `crates/service/src/dashboard.rs`, `crates/service/src/rpc_dispatch/dashboard.rs`.
- Test: `request_activity.rs`, `crates/service/src/dashboard_tests.rs`.

**Interfaces:**
- `begin_request_activity(start: RequestActivityStart) -> RequestActivityGuard`.
- `mark_request_activity_queued(trace_id: &str, route_kind: &str)`.
- `mark_request_activity_running(trace_id: &str, route_kind: &str)`.
- `update_request_activity_source(trace_id: &str, source_kind: Option<&str>, source_id: Option<&str>)`.
- `request_activity_snapshot(limit: usize) -> DashboardActiveRequestsResult`.
- RPC method: `dashboard/activeRequests`, admin-only, default limit 50 and clamp range 1..=50.

- [ ] **Step 1: Write failing tracker tests**

Create entries for two requests from the same IP, mark one running and leave one queued, then assert the snapshot reports:

```rust
assert_eq!(group.total_count, 2);
assert_eq!(group.running_count, 1);
assert_eq!(group.queued_count, 1);
```

Add a guard-drop test that asserts the item disappears after success, error and explicit cancellation cleanup.

- [ ] **Step 2: Run the tests to verify they fail**

Run:

```powershell
cargo test -p codexmanager-service request_activity --lib -- --test-threads=1
```

Expected: FAIL because the tracker and DTOs are absent.

- [ ] **Step 3: Implement the in-memory tracker**

Use a process-local `OnceLock<Mutex<HashMap<trace_id, Entry>>>`. Store `queued`/`running`, client IP, path, method, model, route/source and timestamps. Build IP groups from all entries before truncating detail rows; running rows sort before queued rows.

- [ ] **Step 4: Wire the async request lifetime**

Create the guard immediately after request validation and retain it through `GatewayRequest` completion. Mark queued before IP/fixed gates, running after the last gate, update source when candidate selection succeeds, and rely on RAII/drop plus cancellation paths for cleanup.

- [ ] **Step 5: Expose admin-only Dashboard RPC**

Return `permission_denied` for non-admin actors. Do not read historical request logs for this endpoint; it must be an in-memory snapshot.

- [ ] **Step 6: Rerun backend tracker tests**

Run:

```powershell
cargo test -p codexmanager-service request_activity --lib -- --test-threads=1
cargo test -p codexmanager-service dashboard_tests --lib -- --test-threads=1
```

Expected: PASS, including cleanup and admin permission checks.

- [ ] **Step 7: Commit backend realtime tracking**

Run:

```powershell
git add -- crates/service/src/gateway/observability/request_activity.rs crates/service/src/gateway/mod.rs crates/service/src/http/gateway_endpoint.rs crates/service/src/gateway/request/request_entry.rs crates/service/src/gateway/upstream/proxy_pipeline/candidate_executor.rs crates/core/src/rpc/types.rs crates/service/src/dashboard.rs crates/service/src/rpc_dispatch/dashboard.rs
git commit -m "feat(service): 增加实时请求活动快照"
```

---

### Task 7: Add Desktop Realtime Dashboard And Conditional IP Concurrency

**Files:**
- Modify: `apps/src-tauri/src/commands/dashboard.rs`, `apps/src-tauri/src/commands/registry.rs`, `apps/src/lib/api/dashboard-client.ts`, `apps/src/types/dashboard.ts`, `apps/src/types/index.ts`, `apps/src/app/page.tsx`.
- Create: `apps/src/hooks/useDashboardActiveRequests.ts`, `apps/tests/dashboard-active-requests.test.mjs`.
- Modify: `crates/service/src/gateway/routing/request_gate.rs`, `crates/service/src/gateway/upstream/proxy_pipeline/request_gate.rs`, `crates/service/src/http/gateway_endpoint.rs`: add the conditional IP gate at the current async proxy entry.
- Test: `crates/service/src/gateway/routing/tests/request_gate_tests.rs`, `apps/tests/dashboard-active-requests.test.mjs`.

**Interfaces:**
- Tauri command: `service_dashboard_active_requests(addr, limit)`.
- Frontend hook: `useDashboardActiveRequests(enabled: boolean, isDesktopRuntime: boolean)`.
- Poll interval: 1500 ms only when enabled.
- Gate constant: `CLIENT_IP_GATE_MAX_RUNNING = 4`.
- Gate behavior: active distinct IP count <= 1 permits unlimited same-IP running requests; active distinct IP count >= 2 permits at most 4 per IP.

- [ ] **Step 1: Write failing conditional gate tests**

Test all four required transitions:

```rust
// six A requests are admitted while A is the only active IP
// after B appears, the next A request waits once A already has >= 4
// B has its own four slots
// after B exits, A-only mode admits a waiting A request again
```

Also assert that a request with `None`/blank IP skips only the IP gate.

- [ ] **Step 2: Run the gate tests to verify they fail**

Run:

```powershell
cargo test -p codexmanager-service request_gate_tests --lib -- --test-threads=1
```

Expected: FAIL because the upstream async gate only supports one held request per fixed key and has no conditional IP state.

- [ ] **Step 3: Implement the dynamic IP gate**

Keep the author's existing fixed gate API and async cancellation/deadline behavior. Add a separate `ClientIpGateState` with `running_by_ip` and a condition notification. The admission predicate is:

```text
active_ip_count <= 1
    OR running_by_ip[client_ip] < 4
```

The guard must release the IP count on drop, wake waiters, and return the existing request deadline's timeout/cancellation error rather than creating an unbounded queue. Do not let IP gate errors bypass the author's other request, service, or route gates.

- [ ] **Step 4: Wire IP gate before upstream candidate execution**

When `client_ip` exists, acquire the IP gate before the author fixed key/path/model gate and retain the guard until the downstream response completes. Mark the active request queued/running around waits. When the IP is absent, skip only this gate.

- [ ] **Step 5: Write failing frontend Dashboard contract test**

Assert:

```js
assert.match(commandSource, /service_dashboard_active_requests/);
assert.match(hookSource, /1500/);
assert.match(pageSource, /useDashboardActiveRequests/);
assert.match(pageSource, /运行中/);
assert.match(pageSource, /排队中/);
```

The test must also assert the member Dashboard path does not mount the admin realtime hook.

- [ ] **Step 6: Implement the desktop command, hook and in-place panel**

Normalize snake_case/camelCase active request DTOs. Poll only when desktop runtime, service connected, page visible and current actor is admin. Render existing card/table primitives with IP groups and detail rows; preserve current layout and styles, and show running/queued/total counts.

- [ ] **Step 7: Rerun backend and frontend focused tests**

Run:

```powershell
cargo test -p codexmanager-service request_gate_tests --lib -- --test-threads=1
pnpm -C apps exec node --test tests/dashboard-active-requests.test.mjs
```

Expected: PASS, including single-IP unlimited behavior, multi-IP isolation and desktop-only Dashboard wiring.

- [ ] **Step 8: Commit realtime App and gate**

Run:

```powershell
git add -- apps/src-tauri/src/commands/dashboard.rs apps/src-tauri/src/commands/registry.rs apps/src/lib/api/dashboard-client.ts apps/src/types/dashboard.ts apps/src/types/index.ts apps/src/app/page.tsx apps/src/hooks/useDashboardActiveRequests.ts apps/tests/dashboard-active-requests.test.mjs crates/service/src/gateway/routing/request_gate.rs crates/service/src/gateway/upstream/proxy_pipeline/request_gate.rs crates/service/src/http/gateway_endpoint.rs crates/service/src/gateway/routing/tests/request_gate_tests.rs
git commit -m "feat(app): 增加实时请求监控与条件 IP 并发"
```

---

### Task 8: Regress Author Features, Build, Start And Publish

**Files:**
- Verify all implementation files from Tasks 1-7.
- Modify only when a regression is proven in the current integration branch.
- Do not stage unrelated user files or restore the removed temporary worktree.

**Interfaces:**
- Consumes: author v0.6.2 runtime, model, storage, aggregate API, account and release contracts plus all local feature contracts from Tasks 2-7.
- Produces: a locally identifiable desktop App that builds and starts against the isolated database and can be published through `local`.

- [ ] **Step 1: Run author feature regression tests**

Run:

```powershell
cargo test -p codexmanager-service --lib account_reset_warmup -- --test-threads=1
cargo test -p codexmanager-service --lib model_catalog -- --test-threads=1
cargo test -p codexmanager-service --lib aggregate_api -- --test-threads=1
cargo test -p codexmanager-storage-seaorm --lib -- --test-threads=1
```

Expected: author account wake-up/batch switch, model catalog/price synchronization, direct aggregate API, and SeaORM migration/storage tests pass.

- [ ] **Step 2: Run local backend regressions**

Run:

```powershell
cargo test -p codexmanager-service --lib client_ip -- --test-threads=1
cargo test -p codexmanager-service --lib request_activity -- --test-threads=1
cargo test -p codexmanager-service --lib request_gate_tests -- --test-threads=1
cargo test -p codexmanager-service --lib request_log -- --test-threads=1
```

Expected: trusted IP, IP aggregation, realtime cleanup, and conditional gate tests pass serially.

- [ ] **Step 3: Run App runtime tests and desktop build**

Run:

```powershell
pnpm -C apps run test:runtime
pnpm -C apps run build:desktop
```

Expected: frontend runtime contracts pass and the static desktop bundle is generated successfully.

- [ ] **Step 4: Perform a real local desktop smoke**

Run:

```powershell
pnpm dlx @tauri-apps/cli@2.10.1 dev
```

Verify together:

- the launcher and `CodexManagerLocal` process start;
- the local service endpoint responds;
- a test request creates a request log with the actual client IP;
- the App API Keys page shows one IP row with Token and estimated amount;
- the Dashboard shows running/queued realtime data when requests overlap;
- two active IPs trigger the per-IP four-running-request behavior;
- the App data directory/DB namespace is not the author's identifier.

Stop the launcher and all child processes after the smoke; record the exact process cleanup command and endpoint result.

- [ ] **Step 5: Run final source/config checks**

Run:

```powershell
cargo test --workspace -- --test-threads=1
git diff --check
git grep -n -E 'qxcnm/Codex-Manager|com.codexmanager.desktop|productName.*CodexManager"' -- apps/src-tauri apps/src .github
git status --short
```

Expected: workspace tests pass serially, `git diff --check` exits 0, no local default points back to the author, and only intended files are changed.

- [ ] **Step 6: Merge the validated integration branch into local main**

After review of the final diff:

```powershell
git switch main
git merge --ff-only codexmanagerlocal-v062-integration
```

Expected: `main` contains the author v0.6.2 baseline plus all local functionality, with no direct merge conflict resolution left unverified.

- [ ] **Step 7: Push the local release branch**

Run:

```powershell
git push local main
```

Expected: `local/main` points to the validated local App commit. Do not push the local fork changes to `origin`.

## Completion Criteria

The implementation is complete only when:

1. `origin/main` v0.6.2 author features and async runtime remain available.
2. `CodexManagerLocal` identity, database namespace, updater and release assets are local.
3. IP usage is one row per IP, including Token, estimated amount, request/success/error counts, last-seen and sorting.
4. IP is trusted, persisted and carried through success/error/cancellation paths.
5. Admin desktop Dashboard shows live queued/running requests and IP groups.
6. Single-IP mode has no four-request cap; multi-IP mode caps each IP at four and isolates IPs.
7. `pnpm -C apps run test:runtime`, `pnpm -C apps run build:desktop`, focused Rust tests and the serial workspace validation have actual recorded results.
8. The desktop App is locally started and stopped after a smoke test.
9. Only after verification is the result merged into `main` and pushed to `local`.
