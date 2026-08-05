# ZhangHao Local Features Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore ZhangHao's local CodexManager features on the current upstream version without changing the existing visual style or page layout.

**Scope Update (2026-08-05):** User clarified that only the desktop App is required; service-mode Web UI is out of scope. During execution, skip any `crates/web`, `codexmanager-web`, and `transport-web-commands` tasks or validation steps in the original plan.

**Architecture:** Keep the upstream repository boundaries intact: Tauri/local release work stays under `apps/src-tauri` and `.github`, persistent IP/token data stays in `crates/core`, service/RPC/gateway behavior needed by the desktop App stays in `crates/service`, and frontend App wiring stays in `apps/src`. Preserve raw `key_id + client_ip` storage for permission filtering, but expose and render IP usage as one row per client IP.

**Tech Stack:** Rust, rusqlite, tiny_http, axum, Tauri v2, Next.js 16 static export, React 19, TanStack Query, Node test runner.

---

## Working Rules

- Do not stage or modify unrelated current work: `.gitignore` and `docs/zhanghao-local-features-migration.md` are pre-existing workspace changes.
- Keep UI layout and styling intact. Add fields, hooks, and small in-place sections only where the current pages already have matching surfaces.
- Do not implement or validate service-mode Web UI behavior for this migration.
- For every task, run the focused failing test before implementation, then rerun it after implementation.
- Use `Get-Content -Encoding UTF8` when reading Chinese or mixed-language files.
- Use `apply_patch` for manual edits.
- Commit only the files listed in each task's commit step.

## File Structure

### Local App Identity And Release

- Create `apps/src-tauri/tauri.local.conf.json`: local Tauri overlay for `CodexManagerLocal`.
- Create `.github/workflows/release-local.yml`: Windows/macOS x64 local release workflow.
- Create `assets/macos-local/Open CodexManagerLocal.command`: macOS first-launch helper.
- Create `assets/macos-local/README-macOS-first-launch-local.txt`: macOS launch notes.
- Create `apps/tests/local-release-workflow.test.mjs`: source-level local release regression test.

### Client IP Plumbing

- Create `crates/service/src/client_ip.rs`: trusted internal client IP header helpers.
- Modify `crates/service/src/lib.rs`: export the `client_ip` module.
- Modify `crates/service/src/http/proxy_runtime.rs`: inject peer IP into forwarded service requests.
- Modify `crates/web/src/service_gateway.rs`: strip external IP header and inject Web gateway peer IP.
- Modify `crates/service/src/gateway/request/request_entry.rs`: resolve `client_ip` at tiny_http entry.
- Modify `crates/service/src/gateway/local_validation/mod.rs`: add `client_ip` to `LocalValidationResult`.
- Modify `crates/service/src/gateway/local_validation/request.rs`: carry `client_ip` into successful validation results.

### Storage And Usage

- Modify `crates/core/src/storage/mod.rs`: add `client_ip` to `RequestLog` and `RequestTokenStat` structs if those structs live there.
- Modify `crates/core/src/storage/request_logs.rs`: add `client_ip` column, index, filters, mapping, and query search support.
- Modify `crates/core/src/storage/request_token_stats.rs`: add `client_ip` column, hourly rollup support, and IP aggregation methods.
- Modify `crates/core/src/rpc/types.rs`: add client IP usage and active request result types.
- Modify `crates/service/src/gateway/observability/request_log.rs`: write `client_ip` into request logs and token stats.
- Modify all `RequestLogTraceContext` call sites touched by compiler errors to pass `client_ip` where the current request has it.

### Requestlog RPC And Frontend Usage

- Create `crates/service/src/requestlog/requestlog_client_ip_usage.rs`: service reader for IP usage.
- Modify `crates/service/src/requestlog/mod.rs`: export the new module.
- Modify `crates/service/src/rpc_dispatch/requestlog.rs`: expose `requestlog/client_ip_usage` with existing admin/member filtering.
- Modify `apps/src-tauri/src/commands/requestlog.rs`: add `service_requestlog_client_ip_usage`.
- Modify `apps/src-tauri/src/commands/registry.rs`: register the Tauri command.
- Modify `apps/src/lib/api/service-client.ts`: add `listClientIpUsage()`.
- Modify `apps/src/lib/api/normalize.ts`: normalize client IP usage results.
- Modify `apps/src/lib/api/transport-web-commands/misc.ts`: map the web command to `requestlog/clientIpUsage` or the repo's existing method mapping convention.
- Modify `apps/src/types/request-log.ts` and `apps/src/types/index.ts`: add `clientIp` and `ClientIpUsageSummary`.
- Modify `apps/src/app/logs/page-cells.tsx` and `apps/src/app/logs/page-sections.tsx`: show and search client IP.
- Modify `apps/src/app/apikeys/page.tsx`: show one row per client IP with total and today token totals.

### Active Requests And IP Realtime Dashboard

- Create `crates/service/src/gateway/observability/request_activity.rs`: in-memory active request tracker.
- Modify `crates/service/src/gateway/mod.rs`: export activity helpers and clear activity state in test runtime cleanup.
- Modify `crates/service/src/gateway/upstream/proxy.rs`: begin active request guard and mark queued/running.
- Modify `crates/service/src/gateway/upstream/proxy_pipeline/candidate_executor.rs`: update active request source when a real source is selected.
- Modify `crates/service/src/dashboard.rs`: add `read_active_requests()`.
- Modify `crates/service/src/rpc_dispatch/dashboard.rs`: expose `dashboard/activeRequests`.
- Modify `apps/src-tauri/src/commands/dashboard.rs` and `apps/src-tauri/src/commands/registry.rs`: add/register `service_dashboard_active_requests`.
- Modify `apps/src/lib/api/dashboard-client.ts`: add `getActiveRequests()`.
- Modify `apps/src/types/dashboard.ts` and `apps/src/types/index.ts`: add active request and IP group types.
- Create `apps/src/hooks/useDashboardActiveRequests.ts`: gated polling hook.
- Modify `apps/src/app/page.tsx`: mount active request data in the admin Dashboard only.

### Client IP Gate

- Modify `crates/service/src/gateway/routing/request_gate.rs`: support locks with `max_running` and add `client_ip_gate_lock()`.
- Modify `crates/service/src/gateway/upstream/proxy_pipeline/request_gate.rs`: keep existing request gate behavior and add an IP gate helper if it fits the current module shape.
- Modify `crates/service/src/gateway/upstream/proxy.rs`: acquire the IP gate before candidate execution and update active request state.

---

### Task 1: CodexManagerLocal Identity And Release Fixtures

**Files:**
- Create: `apps/src-tauri/tauri.local.conf.json`
- Create: `.github/workflows/release-local.yml`
- Create: `assets/macos-local/Open CodexManagerLocal.command`
- Create: `assets/macos-local/README-macOS-first-launch-local.txt`
- Create: `apps/tests/local-release-workflow.test.mjs`

- [ ] **Step 1: Write the failing local release test**

Create `apps/tests/local-release-workflow.test.mjs` with these assertions:

```js
import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

const appsRoot = path.resolve(import.meta.dirname, "..");
const repoRoot = path.resolve(appsRoot, "..");
const localConfigPath = path.join(appsRoot, "src-tauri", "tauri.local.conf.json");
const workflowPath = path.join(repoRoot, ".github", "workflows", "release-local.yml");

assert.equal(existsSync(localConfigPath), true);
const localConfig = JSON.parse(readFileSync(localConfigPath, "utf8"));
assert.equal(localConfig.productName, "CodexManagerLocal");
assert.equal(localConfig.identifier, "com.codexmanager.local");
assert.equal(localConfig.app?.windows?.[0]?.title, "CodexManager Local");

assert.equal(existsSync(workflowPath), true);
const workflowSource = readFileSync(workflowPath, "utf8");
assert.match(workflowSource, /^name:\s*release-local/m);
assert.match(workflowSource, /CodexManagerLocal/);
assert.match(workflowSource, /codexmanagerlocal-windows-x64/);
assert.match(workflowSource, /codexmanagerlocal-macos-x64/);
assert.match(workflowSource, /--config\s+src-tauri\/tauri\.local\.conf\.json/);
assert.doesNotMatch(workflowSource, /CodexManager\.app/);
assert.doesNotMatch(workflowSource, /CodexManager_\$\{?version\}?/);
```

- [ ] **Step 2: Run the test and confirm it fails**

Run:

```powershell
pnpm -C apps exec node --test tests/local-release-workflow.test.mjs
```

Expected: FAIL because `apps/src-tauri/tauri.local.conf.json` and `.github/workflows/release-local.yml` do not exist.

- [ ] **Step 3: Add the local Tauri config**

Create `apps/src-tauri/tauri.local.conf.json`:

```json
{
  "productName": "CodexManagerLocal",
  "identifier": "com.codexmanager.local",
  "app": {
    "windows": [
      {
        "title": "CodexManager Local",
        "width": 1100,
        "height": 720,
        "resizable": true,
        "visible": false,
        "create": false
      }
    ]
  }
}
```

- [ ] **Step 4: Add macOS local helper assets**

Create `assets/macos-local/Open CodexManagerLocal.command`:

```bash
#!/usr/bin/env bash
set -euo pipefail

APP="/Applications/CodexManagerLocal.app"
if [ ! -d "$APP" ]; then
  DIR="$(cd "$(dirname "$0")" && pwd)"
  APP="$DIR/CodexManagerLocal.app"
fi

if [ ! -d "$APP" ]; then
  echo "CodexManagerLocal.app not found."
  exit 1
fi

xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
open "$APP"
```

Create `assets/macos-local/README-macOS-first-launch-local.txt`:

```text
CodexManagerLocal macOS first launch

If macOS blocks the app because it is not notarized, run "Open CodexManagerLocal.command" from this DMG.
The helper removes quarantine from CodexManagerLocal.app and opens it.
```

- [ ] **Step 5: Add the local release workflow**

Create `.github/workflows/release-local.yml` by porting the old local workflow and keeping these required details:

```yaml
name: release-local

on:
  workflow_dispatch:
    inputs:
      tag:
        description: "Local release tag"
        required: true
        type: string
      ref:
        description: "Git ref/branch/sha to build"
        required: false
        default: "main"
        type: string

permissions:
  contents: write

env:
  TAURI_CLI_VERSION: 2.10.1
  CARGO_TARGET_DIR: ${{ github.workspace }}/target-local
```

The workflow must build Windows with:

```powershell
pnpm dlx "@tauri-apps/cli@${{ env.TAURI_CLI_VERSION }}" build --bundles nsis --config src-tauri/tauri.local.conf.json --ci
```

The workflow must build macOS x64 with:

```bash
pnpm dlx "@tauri-apps/cli@${{ env.TAURI_CLI_VERSION }}" build --bundles app --target x86_64-apple-darwin --config src-tauri/tauri.local.conf.json --ci
```

Stage artifacts as `CodexManagerLocal_${version}_x64-setup.exe` and `CodexManagerLocal_${version}_x64.dmg`.

- [ ] **Step 6: Rerun the focused test**

Run:

```powershell
pnpm -C apps exec node --test tests/local-release-workflow.test.mjs
```

Expected: PASS.

- [ ] **Step 7: Commit local identity fixtures**

Run:

```powershell
git add -- apps/src-tauri/tauri.local.conf.json .github/workflows/release-local.yml assets/macos-local apps/tests/local-release-workflow.test.mjs
git commit -m "feat(release): 恢复 CodexManagerLocal 本地版配置"
```

---

### Task 2: Trusted Client IP Plumbing

**Files:**
- Create: `crates/service/src/client_ip.rs`
- Modify: `crates/service/src/lib.rs`
- Modify: `crates/service/src/http/proxy_runtime.rs`
- Modify: `crates/web/src/service_gateway.rs`
- Modify: `crates/service/src/gateway/request/request_entry.rs`
- Modify: `crates/service/src/gateway/local_validation/mod.rs`
- Modify: `crates/service/src/gateway/local_validation/request.rs`
- Test: `crates/service/src/http/tests/proxy_runtime_tests.rs`
- Test: `crates/web/src/service_gateway_tests.rs`

- [ ] **Step 1: Write service client IP unit tests**

Create tests in `crates/service/src/client_ip.rs` under `#[cfg(test)]`:

```rust
#[test]
fn loopback_remote_trusts_forwarded_client_ip() {
    let remote = "127.0.0.1:48760".parse().unwrap();
    assert_eq!(
        resolve_trusted_client_ip(Some(&remote), Some("192.168.1.20")),
        Some("192.168.1.20".to_string())
    );
}

#[test]
fn non_loopback_remote_ignores_forwarded_client_ip() {
    let remote = "10.0.0.10:48760".parse().unwrap();
    assert_eq!(
        resolve_trusted_client_ip(Some(&remote), Some("192.168.1.20")),
        Some("10.0.0.10".to_string())
    );
}
```

- [ ] **Step 2: Run the service client IP tests and confirm they fail**

Run:

```powershell
cargo test -p codexmanager-service client_ip -- --test-threads=1
```

Expected: FAIL because `crates/service/src/client_ip.rs` does not exist or is not exported.

- [ ] **Step 3: Implement the client IP helper**

Create `crates/service/src/client_ip.rs`:

```rust
use std::net::{IpAddr, SocketAddr};

use axum::http::{HeaderMap, HeaderValue};

pub const FORWARDED_CLIENT_IP_HEADER: &str = "x-codexmanager-client-ip";

fn parse_forwarded_client_ip(value: &str) -> Option<IpAddr> {
    value
        .split(',')
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .and_then(|value| value.parse::<IpAddr>().ok())
}

pub fn resolve_trusted_client_ip(
    remote_addr: Option<&SocketAddr>,
    forwarded_client_ip: Option<&str>,
) -> Option<String> {
    let remote_ip = remote_addr.map(SocketAddr::ip)?;
    if remote_ip.is_loopback() {
        if let Some(forwarded_ip) = forwarded_client_ip.and_then(parse_forwarded_client_ip) {
            return Some(forwarded_ip.to_string());
        }
    }
    Some(remote_ip.to_string())
}

pub fn set_forwarded_client_ip_header(headers: &mut HeaderMap, peer_addr: SocketAddr) {
    headers.remove(FORWARDED_CLIENT_IP_HEADER);
    if let Ok(value) = HeaderValue::from_str(&peer_addr.ip().to_string()) {
        headers.insert(FORWARDED_CLIENT_IP_HEADER, value);
    }
}
```

Export it from `crates/service/src/lib.rs`:

```rust
pub mod client_ip;
```

- [ ] **Step 4: Inject the trusted header in HTTP/Web gateways**

In `crates/service/src/http/proxy_runtime.rs`, when building forwarded headers from the Axum request, call:

```rust
crate::client_ip::set_forwarded_client_ip_header(&mut headers, peer_addr);
```

In `crates/web/src/service_gateway.rs`, remove any external header with the same name before forwarding, then set it from the Web gateway peer address:

```rust
headers.remove(codexmanager_service::client_ip::FORWARDED_CLIENT_IP_HEADER);
codexmanager_service::client_ip::set_forwarded_client_ip_header(&mut headers, peer_addr);
```

- [ ] **Step 5: Carry `client_ip` through local validation**

Add to `LocalValidationResult` in `crates/service/src/gateway/local_validation/mod.rs`:

```rust
pub(super) client_ip: Option<String>,
```

In `request_entry.rs`, resolve the IP before validation:

```rust
let client_ip = super::client_ip::resolve_trusted_client_ip(
    request.remote_addr(),
    request
        .headers()
        .iter()
        .find(|header| header.field.as_str().eq_ignore_ascii_case(
            crate::client_ip::FORWARDED_CLIENT_IP_HEADER,
        ))
        .map(|header| header.value.as_str()),
);
```

Pass `client_ip.clone()` into `prepare_local_request()` and copy it into every successful `LocalValidationResult` in `local_validation/request.rs`.

- [ ] **Step 6: Rerun focused tests**

Run:

```powershell
cargo test -p codexmanager-service client_ip -- --test-threads=1
cargo test -p codexmanager-service http::tests::proxy_runtime_tests -- --test-threads=1
cargo test -p codexmanager-web service_gateway -- --test-threads=1
```

Expected: PASS for the focused IP plumbing tests.

- [ ] **Step 7: Commit trusted IP plumbing**

Run:

```powershell
git add -- crates/service/src/client_ip.rs crates/service/src/lib.rs crates/service/src/http/proxy_runtime.rs crates/web/src/service_gateway.rs crates/service/src/gateway/request/request_entry.rs crates/service/src/gateway/local_validation/mod.rs crates/service/src/gateway/local_validation/request.rs crates/service/src/http/tests/proxy_runtime_tests.rs crates/web/src/service_gateway_tests.rs
git commit -m "feat(gateway): 增加可信客户端 IP 传递"
```

---

### Task 3: Store Client IP In Request Logs And Token Stats

**Files:**
- Modify: `crates/core/src/storage/mod.rs`
- Modify: `crates/core/src/storage/request_logs.rs`
- Modify: `crates/core/src/storage/request_token_stats.rs`
- Modify: `crates/service/src/gateway/observability/request_log.rs`
- Test: `crates/core/src/storage/tests/request_logs_tests.rs`
- Test: `crates/core/src/storage/tests/request_token_stats_tests.rs`
- Test: `crates/service/src/gateway/observability/tests/request_log_tests.rs`

- [ ] **Step 1: Add failing storage tests**

In `request_logs_tests.rs`, add a test that inserts a `RequestLog` with `client_ip: Some("192.168.1.20".to_string())`, lists logs with query `"192.168.1.20"`, and asserts one matching item has that `client_ip`.

In `request_token_stats_tests.rs`, add a test that inserts two stats for the same IP across two keys and asserts the new IP rollup totals both rows:

```rust
let items = storage
    .summarize_request_token_stats_by_client_ip_between(start_ts, end_ts, None)
    .expect("summarize by client ip");
assert_eq!(items.len(), 1);
assert_eq!(items[0].client_ip, "192.168.1.20");
assert_eq!(items[0].usage.total_tokens, 300);
```

- [ ] **Step 2: Run the focused storage tests and confirm they fail**

Run:

```powershell
cargo test -p codexmanager-core request_logs_tests:: -- --test-threads=1
cargo test -p codexmanager-core request_token_stats_tests:: -- --test-threads=1
```

Expected: FAIL because `client_ip` fields and IP rollup methods do not exist.

- [ ] **Step 3: Add `client_ip` to storage structs and schema**

Add nullable `client_ip` fields to the relevant storage structs:

```rust
pub client_ip: Option<String>,
```

In `request_logs.rs`, ensure schema includes:

```sql
client_ip TEXT
```

and an index:

```sql
CREATE INDEX IF NOT EXISTS idx_request_logs_client_ip_created_at
ON request_logs(client_ip, created_at DESC)
```

Include `client_ip` in insert, select, row mapping, and query search conditions.

- [ ] **Step 4: Add `client_ip` to token stats and hourly rollups**

In `request_token_stats.rs`, add `client_ip` to raw stats and hourly rollups:

```sql
client_ip TEXT
```

For `request_token_stat_hourly_rollups`, include `client_ip TEXT NOT NULL DEFAULT ''` and update the primary key to include `client_ip`:

```sql
PRIMARY KEY(bucket_start, key_id, client_ip, account_id, model, actual_source_kind, actual_source_id, owner_user_id)
```

Backfill raw token stats from `request_logs.client_ip` when historical request logs have the column.

- [ ] **Step 5: Add IP aggregation method**

Add a storage method with this public shape:

```rust
pub fn summarize_request_token_stats_by_client_ip_between(
    &self,
    start_ts: i64,
    end_ts: i64,
    key_ids: Option<&[String]>,
) -> Result<Vec<ClientIpUsageSummary>>
```

The method must combine raw stats and hourly rollups, filter by `key_ids` when provided, group by `client_ip`, sort by `total_tokens DESC, client_ip ASC`, and omit blank IP values.

- [ ] **Step 6: Write `client_ip` from gateway request logs**

Add `client_ip` to `RequestLogTraceContext<'_>`:

```rust
pub(crate) client_ip: Option<&'a str>,
```

When creating `RequestLog` and `RequestTokenStat`, copy:

```rust
client_ip: trace_context.client_ip.map(str::to_string),
```

Pass `client_ip` into the trace context for validated gateway paths and model/aggregate/precheck error paths.

- [ ] **Step 7: Rerun storage and request log tests**

Run:

```powershell
cargo test -p codexmanager-core request_logs_tests:: -- --test-threads=1
cargo test -p codexmanager-core request_token_stats_tests:: -- --test-threads=1
cargo test -p codexmanager-service gateway::observability::tests::request_log_tests -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 8: Commit storage support**

Run:

```powershell
git add -- crates/core/src/storage/mod.rs crates/core/src/storage/request_logs.rs crates/core/src/storage/request_token_stats.rs crates/core/src/storage/tests/request_logs_tests.rs crates/core/src/storage/tests/request_token_stats_tests.rs crates/service/src/gateway/observability/request_log.rs crates/service/src/gateway/observability/tests/request_log_tests.rs
git commit -m "feat(storage): 记录客户端 IP 用量"
```

---

### Task 4: Client IP Usage RPC Aggregated By IP

**Files:**
- Modify: `crates/core/src/rpc/types.rs`
- Create: `crates/service/src/requestlog/requestlog_client_ip_usage.rs`
- Modify: `crates/service/src/requestlog/mod.rs`
- Modify: `crates/service/src/rpc_dispatch/requestlog.rs`
- Modify: `apps/src-tauri/src/commands/requestlog.rs`
- Modify: `apps/src-tauri/src/commands/registry.rs`
- Test: `crates/service/src/requestlog/requestlog_client_ip_usage_tests.rs`

- [ ] **Step 1: Add failing service tests**

Create `crates/service/src/requestlog/requestlog_client_ip_usage_tests.rs` with tests for:

```rust
#[test]
fn client_ip_usage_merges_multiple_keys_into_one_ip_row() {
    // Insert stats for key-a and key-b with the same client_ip.
    // Call read_client_ip_usage_with_storage(&storage, params, None).
    // Assert one result row and no key_id field in the result type.
}

#[test]
fn client_ip_usage_respects_member_key_filter_before_ip_merge() {
    // Insert key-a and key-b for the same client_ip.
    // Call with Some(&["key-a".to_string()]).
    // Assert totals include only key-a.
}
```

- [ ] **Step 2: Run the service tests and confirm they fail**

Run:

```powershell
cargo test -p codexmanager-service requestlog_client_ip_usage -- --test-threads=1
```

Expected: FAIL because the module and RPC types do not exist.

- [ ] **Step 3: Add RPC types without key-level display**

In `crates/core/src/rpc/types.rs`, add:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientIpUsageListParams {
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientIpUsageSummaryResult {
    pub client_ip: String,
    pub request_count: i64,
    pub success_count: i64,
    pub error_count: i64,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
    pub total_tokens: i64,
    pub estimated_cost_usd: f64,
    pub last_seen_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientIpUsageListResult {
    pub items: Vec<ClientIpUsageSummaryResult>,
}
```

Do not include `key_id` in `ClientIpUsageSummaryResult`.

- [ ] **Step 4: Implement requestlog client IP usage reader**

Create `requestlog_client_ip_usage.rs` with:

```rust
pub(crate) fn read_client_ip_usage_with_storage(
    storage: &Storage,
    params: ClientIpUsageListParams,
    key_ids: Option<&[String]>,
) -> Result<ClientIpUsageListResult, String> {
    let params = NormalizedClientIpUsageParams::from_params(params);
    if params.limit == 0 || key_ids.is_some_and(|ids| ids.is_empty()) {
        return Ok(ClientIpUsageListResult::default());
    }
    let rows = storage
        .summarize_request_token_stats_by_client_ip_between(
            params.start_ts,
            params.end_ts,
            key_ids,
        )
        .map_err(|err| format!("summarize client ip usage failed: {err}"))?;
    Ok(ClientIpUsageListResult {
        items: rows.into_iter().take(params.limit).map(map_client_ip_usage_summary).collect(),
    })
}
```

- [ ] **Step 5: Expose RPC and Tauri command**

In `requestlog.rs`, add a match arm:

```rust
"requestlog/client_ip_usage" => {
    let params = req.params.clone()
        .map(serde_json::from_value::<ClientIpUsageListParams>)
        .transpose()
        .map(|params| params.unwrap_or_default())
        .map_err(|err| format!("invalid requestlog/client_ip_usage params: {err}"));
    super::value_or_error(params.and_then(|params| {
        if actor.is_admin() {
            requestlog_client_ip_usage::read_client_ip_usage(params, None)
        } else {
            let (storage, key_ids) = member_requestlog_scope(actor)?;
            requestlog_client_ip_usage::read_client_ip_usage_with_storage(&storage, params, Some(&key_ids))
        }
    }))
}
```

In Tauri requestlog commands, add:

```rust
#[tauri::command]
pub async fn service_requestlog_client_ip_usage(
    addr: Option<String>,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({ "startTs": start_ts, "endTs": end_ts, "limit": limit });
    rpc_call_in_background("requestlog/client_ip_usage", addr, Some(params)).await
}
```

Register it in `apps/src-tauri/src/commands/registry.rs`.

- [ ] **Step 6: Rerun focused tests**

Run:

```powershell
cargo test -p codexmanager-service requestlog_client_ip_usage -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 7: Commit IP usage RPC**

Run:

```powershell
git add -- crates/core/src/rpc/types.rs crates/service/src/requestlog/requestlog_client_ip_usage.rs crates/service/src/requestlog/mod.rs crates/service/src/rpc_dispatch/requestlog.rs crates/service/src/requestlog/requestlog_client_ip_usage_tests.rs apps/src-tauri/src/commands/requestlog.rs apps/src-tauri/src/commands/registry.rs
git commit -m "feat(requestlog): 按客户端 IP 汇总用量"
```

---

### Task 5: Logs And API Keys Frontend Wiring

**Files:**
- Modify: `apps/src/types/request-log.ts`
- Modify: `apps/src/types/index.ts`
- Modify: `apps/src/lib/api/normalize.ts`
- Modify: `apps/src/lib/api/service-client.ts`
- Modify: `apps/src/lib/api/transport-web-commands/misc.ts`
- Modify: `apps/src/app/logs/page-cells.tsx`
- Modify: `apps/src/app/logs/page-sections.tsx`
- Modify: `apps/src/app/apikeys/page.tsx`
- Test: `apps/tests/request-log-client-ip-normalize.test.mjs`
- Test: `apps/tests/request-log-client-ip-ui.test.mjs`
- Test: `apps/tests/apikey-client-ip-usage.test.mjs`
- Test: `apps/tests/transport-web-commands.test.mjs`
- Test: `apps/tests/tauri-command-registry.test.mjs`

- [ ] **Step 1: Add failing frontend runtime tests**

Create `apps/tests/request-log-client-ip-normalize.test.mjs` to import or source-check `normalizeRequestLogListResult()` and assert `client_ip` maps to `clientIp`.

Create `apps/tests/apikey-client-ip-usage.test.mjs` as a source test asserting:

```js
assert.match(source, /listClientIpUsage/);
assert.match(source, /todayTokensByClientIp/);
assert.doesNotMatch(source, /keyId:\s*item\.keyId/);
assert.match(source, /clientIp/);
```

Create `apps/tests/request-log-client-ip-ui.test.mjs` asserting `page-sections.tsx` contains `客户端 IP` and search text mentions IP.

- [ ] **Step 2: Run the frontend tests and confirm they fail**

Run:

```powershell
pnpm -C apps exec node --test tests/request-log-client-ip-normalize.test.mjs tests/request-log-client-ip-ui.test.mjs tests/apikey-client-ip-usage.test.mjs
```

Expected: FAIL because types, normalize, API client, and UI references do not exist.

- [ ] **Step 3: Add frontend types and normalization**

Add to `apps/src/types/request-log.ts`:

```ts
export interface ClientIpUsageSummary {
  clientIp: string;
  requestCount: number;
  successCount: number;
  errorCount: number;
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningOutputTokens: number;
  totalTokens: number;
  todayTokens?: number;
  estimatedCostUsd: number;
  lastSeenAt: number;
}

export interface ClientIpUsageListResult {
  items: ClientIpUsageSummary[];
}
```

Add `clientIp?: string | null` to the request log summary type.

In `normalize.ts`, add a `normalizeClientIpUsageListResult()` that reads both snake_case and camelCase and never returns `keyId`.

- [ ] **Step 4: Add service client and web command mapping**

In `service-client.ts`, add:

```ts
async listClientIpUsage(params?: {
  startTs?: number | null;
  endTs?: number | null;
  limit?: number | null;
}): Promise<ClientIpUsageListResult> {
  const result = await invoke<unknown>(
    "service_requestlog_client_ip_usage",
    withAddr({
      startTs: params?.startTs ?? null,
      endTs: params?.endTs ?? null,
      limit: params?.limit ?? null,
    }),
  );
  return normalizeClientIpUsageListResult(result);
}
```

Map `service_requestlog_client_ip_usage` to the matching RPC method in `transport-web-commands/misc.ts`.

- [ ] **Step 5: Show client IP in Logs**

Add a `ClientIpCell` in `page-cells.tsx` or inline with existing cell patterns. Add a `客户端 IP` column in `page-sections.tsx` and include IP in the search placeholder.

- [ ] **Step 6: Show IP total and today tokens in API Keys**

In `apps/src/app/apikeys/page.tsx`, load two ranges:

```ts
const [clientIpUsage, todayClientIpUsage] = await Promise.all([
  serviceClient.listClientIpUsage({ limit: 100 }),
  serviceClient.listClientIpUsage({ startTs: todayStartTs, endTs: todayEndTs, limit: 100 }),
]);
```

Merge today tokens by `clientIp`:

```ts
const todayTokensByClientIp = new Map(
  todayClientIpUsage.items.map((item) => [item.clientIp, item.totalTokens]),
);
const rows = clientIpUsage.items.map((item) => ({
  ...item,
  todayTokens: todayTokensByClientIp.get(item.clientIp) ?? 0,
}));
```

Do not render or group by `keyId`.

- [ ] **Step 7: Rerun frontend runtime tests**

Run:

```powershell
pnpm -C apps exec node --test tests/request-log-client-ip-normalize.test.mjs tests/request-log-client-ip-ui.test.mjs tests/apikey-client-ip-usage.test.mjs tests/transport-web-commands.test.mjs tests/tauri-command-registry.test.mjs
```

Expected: PASS.

- [ ] **Step 8: Commit frontend IP usage wiring**

Run:

```powershell
git add -- apps/src/types/request-log.ts apps/src/types/index.ts apps/src/lib/api/normalize.ts apps/src/lib/api/service-client.ts apps/src/lib/api/transport-web-commands/misc.ts apps/src/app/logs/page-cells.tsx apps/src/app/logs/page-sections.tsx apps/src/app/apikeys/page.tsx apps/tests/request-log-client-ip-normalize.test.mjs apps/tests/request-log-client-ip-ui.test.mjs apps/tests/apikey-client-ip-usage.test.mjs apps/tests/transport-web-commands.test.mjs apps/tests/tauri-command-registry.test.mjs
git commit -m "feat(ui): 展示客户端 IP 用量汇总"
```

---

### Task 6: Backend Active Requests And Realtime IP Groups

**Files:**
- Create: `crates/service/src/gateway/observability/request_activity.rs`
- Modify: `crates/service/src/gateway/mod.rs`
- Modify: `crates/service/src/gateway/upstream/proxy.rs`
- Modify: `crates/service/src/gateway/upstream/proxy_pipeline/candidate_executor.rs`
- Modify: `crates/service/src/dashboard.rs`
- Modify: `crates/service/src/rpc_dispatch/dashboard.rs`
- Modify: `crates/core/src/rpc/types.rs`
- Test: `crates/service/src/dashboard_tests.rs`
- Test: `crates/service/src/gateway/observability/request_activity.rs`

- [ ] **Step 1: Add failing active request tests**

In `request_activity.rs`, add tests for guard cleanup and IP group aggregation:

```rust
#[test]
fn snapshot_groups_running_and_queued_counts_by_client_ip() {
    clear_request_activity_for_tests();
    let _a = begin_request_activity(RequestActivityStart { trace_id: "a", client_ip: Some("192.168.1.20"), key_id: "key-a", path: "/v1/responses", method: "POST", model: Some("gpt-5") });
    let _b = begin_request_activity(RequestActivityStart { trace_id: "b", client_ip: Some("192.168.1.20"), key_id: "key-b", path: "/v1/responses", method: "POST", model: Some("gpt-5") });
    mark_request_activity_running("a", "client_ip_gate");
    let snapshot = request_activity_snapshot(50);
    let group = snapshot.ip_groups.iter().find(|item| item.client_ip == "192.168.1.20").unwrap();
    assert_eq!(group.running_count, 1);
    assert_eq!(group.queued_count, 1);
    assert_eq!(group.total_count, 2);
}
```

In `dashboard_tests.rs`, add tests that admin can read active requests and non-admin receives `permission_denied`.

- [ ] **Step 2: Run active request tests and confirm they fail**

Run:

```powershell
cargo test -p codexmanager-service request_activity -- --test-threads=1
cargo test -p codexmanager-service dashboard_tests:: -- --test-threads=1
```

Expected: FAIL because active request types and RPC are missing.

- [ ] **Step 3: Add active request RPC types**

In `types.rs`, add:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardActiveRequestItem {
    pub id: String,
    pub trace_id: String,
    pub status: String,
    pub client_ip: Option<String>,
    pub key_id: String,
    pub path: String,
    pub method: String,
    pub model: Option<String>,
    pub route_kind: String,
    pub source_kind: Option<String>,
    pub source_id: Option<String>,
    pub created_at_ms: i64,
    pub queued_at_ms: Option<i64>,
    pub running_at_ms: Option<i64>,
    pub wait_ms: i64,
    pub running_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardActiveRequestIpGroup {
    pub client_ip: String,
    pub total_count: i64,
    pub queued_count: i64,
    pub running_count: i64,
    pub max_wait_ms: i64,
    pub max_running_ms: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DashboardActiveRequestsResult {
    pub total_count: i64,
    pub queued_count: i64,
    pub running_count: i64,
    pub items: Vec<DashboardActiveRequestItem>,
    pub ip_groups: Vec<DashboardActiveRequestIpGroup>,
}
```

- [ ] **Step 4: Implement request activity tracker**

Create `request_activity.rs` using a `OnceLock<Mutex<HashMap<String, RequestActivityEntry>>>`. `request_activity_snapshot(limit)` must:

- Sort running before queued.
- Limit `items` to the requested limit.
- Build `ip_groups` from all entries before item truncation.
- Omit blank IP groups.

- [ ] **Step 5: Wire active request lifecycle**

In `proxy_validated_request()`, create a guard immediately after validation destructuring:

```rust
let _activity_guard = super::super::begin_request_activity(
    super::super::request_activity::RequestActivityStart {
        trace_id: trace_id.as_str(),
        client_ip: client_ip.as_deref(),
        key_id: key_id.as_str(),
        path: path.as_str(),
        method: request_method.as_str(),
        model: model_for_log.as_deref(),
    },
);
```

Mark queued before waiting on gates and running after gate acquisition:

```rust
super::super::mark_request_activity_queued(trace_id.as_str(), "request_gate");
super::super::mark_request_activity_running(trace_id.as_str(), "request_gate");
```

In `candidate_executor.rs`, when a source is selected, call:

```rust
crate::gateway::update_request_activity_source(trace_id, source_kind, source_id);
```

- [ ] **Step 6: Expose dashboard RPC**

In `dashboard.rs`, add:

```rust
pub(crate) fn read_active_requests(
    actor: &RpcActor,
    limit: Option<i64>,
) -> Result<DashboardActiveRequestsResult, String> {
    if !actor.is_admin() {
        return Err("permission_denied: active requests require admin session".to_string());
    }
    let limit = limit.unwrap_or(50).clamp(1, 50) as usize;
    Ok(crate::gateway::request_activity_snapshot(limit))
}
```

In dashboard dispatch, add `dashboard/activeRequests`.

- [ ] **Step 7: Rerun backend active request tests**

Run:

```powershell
cargo test -p codexmanager-service request_activity -- --test-threads=1
cargo test -p codexmanager-service dashboard_tests:: -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 8: Commit backend active requests**

Run:

```powershell
git add -- crates/service/src/gateway/observability/request_activity.rs crates/service/src/gateway/mod.rs crates/service/src/gateway/upstream/proxy.rs crates/service/src/gateway/upstream/proxy_pipeline/candidate_executor.rs crates/service/src/dashboard.rs crates/service/src/rpc_dispatch/dashboard.rs crates/core/src/rpc/types.rs crates/service/src/dashboard_tests.rs
git commit -m "feat(dashboard): 增加实时 IP 请求监控"
```

---

### Task 7: Dashboard Active Requests Frontend

**Files:**
- Modify: `apps/src-tauri/src/commands/dashboard.rs`
- Modify: `apps/src-tauri/src/commands/registry.rs`
- Modify: `apps/src/lib/api/dashboard-client.ts`
- Modify: `apps/src/lib/api/transport-web-commands/misc.ts`
- Modify: `apps/src/types/dashboard.ts`
- Modify: `apps/src/types/index.ts`
- Create: `apps/src/hooks/useDashboardActiveRequests.ts`
- Modify: `apps/src/app/page.tsx`
- Test: `apps/tests/dashboard-active-requests.test.mjs`
- Test: `apps/tests/tauri-command-registry.test.mjs`
- Test: `apps/tests/transport-web-commands.test.mjs`

- [ ] **Step 1: Add failing frontend dashboard test**

Create `apps/tests/dashboard-active-requests.test.mjs` asserting:

```js
assert.match(clientSource, /service_dashboard_active_requests/);
assert.match(hookSource, /refetchInterval:\s*isQueryEnabled\s*\?\s*1500\s*:\s*false/);
assert.match(adminDashboardSource, /useDashboardActiveRequests/);
assert.doesNotMatch(memberDashboardSource, /useDashboardActiveRequests/);
assert.match(activeRequestsCardSource, /ipGroups/);
assert.match(activeRequestsCardSource, /运行中/);
assert.match(activeRequestsCardSource, /排队中/);
```

- [ ] **Step 2: Run the dashboard frontend test and confirm it fails**

Run:

```powershell
pnpm -C apps exec node --test tests/dashboard-active-requests.test.mjs
```

Expected: FAIL because the hook/client/card do not exist.

- [ ] **Step 3: Add Tauri command and dashboard client**

In `apps/src-tauri/src/commands/dashboard.rs`, add `service_dashboard_active_requests` calling `dashboard/activeRequests`.

In `dashboard-client.ts`, add:

```ts
async getActiveRequests(params?: { limit?: number | null }): Promise<DashboardActiveRequests> {
  const result = await invoke<unknown>(
    "service_dashboard_active_requests",
    withAddr({ limit: params?.limit ?? null }),
  );
  return readActiveRequests(result);
}
```

`readActiveRequests()` must read `items` and `ipGroups` from snake_case or camelCase.

- [ ] **Step 4: Add frontend types and polling hook**

Create `apps/src/hooks/useDashboardActiveRequests.ts`:

```ts
"use client";

import { useQuery } from "@tanstack/react-query";
import { dashboardClient } from "@/lib/api/dashboard-client";

export function useDashboardActiveRequests(
  enabled: boolean,
  isDesktopRuntime: boolean,
) {
  const isQueryEnabled = enabled && isDesktopRuntime;
  return useQuery({
    queryKey: ["dashboard", "activeRequests"],
    queryFn: () => dashboardClient.getActiveRequests({ limit: 50 }),
    enabled: isQueryEnabled,
    refetchInterval: isQueryEnabled ? 1500 : false,
  });
}
```

Use the existing `useRuntimeCapabilities()` hook in `apps/src/app/page.tsx` to read `isDesktopRuntime`, then pass that boolean into `useDashboardActiveRequests(enabled, isDesktopRuntime)`.

- [ ] **Step 5: Render in admin Dashboard only**

In `apps/src/app/page.tsx`, add an `AdminActiveRequestsCard` near the existing admin cards. It must:

- Use existing card/table primitives.
- Render `ipGroups` with client IP, running, queued, total.
- Render active request items with status, IP, model/path, wait/running time.
- Keep a local status filter: `"all" | "running" | "queued"`.
- Return an empty state text `暂无进行中的请求` when there are no items.
- Not be mounted in `MemberDashboard`.

- [ ] **Step 6: Rerun frontend dashboard tests**

Run:

```powershell
pnpm -C apps exec node --test tests/dashboard-active-requests.test.mjs tests/tauri-command-registry.test.mjs tests/transport-web-commands.test.mjs
```

Expected: PASS.

- [ ] **Step 7: Commit frontend active requests**

Run:

```powershell
git add -- apps/src-tauri/src/commands/dashboard.rs apps/src-tauri/src/commands/registry.rs apps/src/lib/api/dashboard-client.ts apps/src/lib/api/transport-web-commands/misc.ts apps/src/types/dashboard.ts apps/src/types/index.ts apps/src/hooks/useDashboardActiveRequests.ts apps/src/app/page.tsx apps/tests/dashboard-active-requests.test.mjs apps/tests/tauri-command-registry.test.mjs apps/tests/transport-web-commands.test.mjs
git commit -m "feat(ui): 展示实时 IP 请求"
```

---

### Task 8: Client IP Concurrency Gate

**Files:**
- Modify: `crates/service/src/gateway/routing/request_gate.rs`
- Modify: `crates/service/src/gateway/upstream/proxy_pipeline/request_gate.rs`
- Modify: `crates/service/src/gateway/upstream/proxy.rs`
- Test: `crates/service/src/gateway/routing/tests/request_gate_tests.rs`

- [ ] **Step 1: Add failing request gate tests**

In `request_gate_tests.rs`, add:

```rust
#[test]
fn client_ip_gate_allows_four_running_requests() {
    clear_request_gate_locks_for_tests();
    let lock = client_ip_gate_lock("192.168.1.20");
    let a = lock.try_acquire().unwrap().expect("first");
    let b = lock.try_acquire().unwrap().expect("second");
    let c = lock.try_acquire().unwrap().expect("third");
    let d = lock.try_acquire().unwrap().expect("fourth");
    assert!(lock.try_acquire().unwrap().is_none());
    drop(a);
    assert!(lock.try_acquire().unwrap().is_some());
    drop((b, c, d));
}

#[test]
fn client_ip_gate_isolated_by_ip() {
    clear_request_gate_locks_for_tests();
    let left = client_ip_gate_lock("192.168.1.20");
    let right = client_ip_gate_lock("192.168.1.21");
    let _left_guard = left.try_acquire().unwrap().expect("left");
    assert!(right.try_acquire().unwrap().is_some());
}
```

- [ ] **Step 2: Run request gate tests and confirm they fail**

Run:

```powershell
cargo test -p codexmanager-service request_gate_tests -- --test-threads=1
```

Expected: FAIL because `client_ip_gate_lock()` and multi-running locks do not exist.

- [ ] **Step 3: Extend request gate state to count running holders**

Change state:

```rust
#[derive(Default)]
struct RequestGateState {
    running: usize,
}
```

Add `max_running` to `RequestGateLock`, keep `RequestGateLock::new()` as one-running default, and add:

```rust
fn with_max_running(max_running: usize) -> Self {
    Self {
        state: Mutex::new(RequestGateState::default()),
        available: Condvar::new(),
        max_running: max_running.max(1),
    }
}
```

`try_acquire()` should return `None` when `state.running >= self.max_running`; otherwise increment `running`. `Drop` should decrement `running` and notify one waiter.

- [ ] **Step 4: Add client IP gate lock**

Add:

```rust
const CLIENT_IP_GATE_MAX_RUNNING: usize = 4;

fn client_ip_gate_key(client_ip: &str) -> String {
    format!("client_ip|{}", client_ip.trim())
}

pub(crate) fn client_ip_gate_lock(client_ip: &str) -> Arc<RequestGateLock> {
    request_gate_lock_for_key(
        client_ip_gate_key(client_ip),
        CLIENT_IP_GATE_MAX_RUNNING,
    )
}
```

Refactor existing `request_gate_lock()` to call the shared `request_gate_lock_for_key(key, 1)`.

- [ ] **Step 5: Acquire client IP gate in proxy**

Before the existing key/path/model request gate in `proxy_validated_request()`, add:

```rust
let _client_ip_gate_guard = if let Some(client_ip) = client_ip.as_deref() {
    acquire_client_ip_request_gate(trace_id.as_str(), client_ip, request_deadline)?
} else {
    None
};
```

The helper should mark active request queued with route kind `client_ip_gate` before waiting and running after acquisition. Timeout returns 504 with message `client IP request queue wait timeout`; poisoned lock returns 503 with message `client IP request gate unavailable`.

- [ ] **Step 6: Rerun request gate tests**

Run:

```powershell
cargo test -p codexmanager-service request_gate_tests -- --test-threads=1
```

Expected: PASS.

- [ ] **Step 7: Commit client IP gate**

Run:

```powershell
git add -- crates/service/src/gateway/routing/request_gate.rs crates/service/src/gateway/upstream/proxy_pipeline/request_gate.rs crates/service/src/gateway/upstream/proxy.rs crates/service/src/gateway/routing/tests/request_gate_tests.rs
git commit -m "feat(gateway): 按客户端 IP 控制并发"
```

---

### Task 9: Full Validation And Final Review

**Files:**
- Verify all files changed in Tasks 1-8.
- Do not stage unrelated `.gitignore` or `docs/zhanghao-local-features-migration.md`.

- [ ] **Step 1: Run Rust validation**

Run:

```powershell
cargo test -p codexmanager-core -- --test-threads=1
cargo test -p codexmanager-service -- --test-threads=1
cargo test -p codexmanager-web -- --test-threads=1
```

Expected: PASS, or record the exact failing test and error text before fixing.

- [ ] **Step 2: Run frontend runtime validation**

Run:

```powershell
pnpm -C apps run test:runtime
```

Expected: PASS.

- [ ] **Step 3: Run desktop build validation**

Run:

```powershell
pnpm -C apps run build:desktop
```

Expected: PASS and `apps/out/index.html` exists.

- [ ] **Step 4: Run diff hygiene check**

Run:

```powershell
git diff --check
git status --short
```

Expected: `git diff --check` exits 0. `git status --short` may still show pre-existing unrelated `.gitignore` and `docs/zhanghao-local-features-migration.md`; report them explicitly.

- [ ] **Step 5: Prepare final summary**

Summarize:

- `CodexManagerLocal` app identity and database isolation.
- IP Token usage is one row per IP, not per key.
- Dashboard realtime IP request groups show running/queued/total.
- Client IP gate limits same-IP concurrency to four.
- Validation commands and results.
