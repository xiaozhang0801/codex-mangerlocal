# Client IP Usage Monitoring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Record and display usage by direct LAN client IP for requests that share the same platform API key.

**Architecture:** Capture the TCP peer IP at the `tiny_http::Request` gateway entrypoint, carry it through local validation into the centralized request-log writer, persist it on request logs and token stats, and expose grouped `key_id + client_ip` usage through requestlog RPC. The frontend adds the IP to request logs and a dense API-key-page usage table without changing existing key totals or trusting proxy headers.

**Tech Stack:** Rust, Tauri v2 commands, SQLite via rusqlite, serde camelCase RPC types, Next.js 16 static export, React 19, TanStack Query, Tailwind/shadcn-style UI.

---

## Constraints And Decisions

- Do not start or stop the user's local app, Tauri shell, Next dev server, or CodexManager service.
- Use build/test commands only; no commands that bind service ports.
- IP source is only `tiny_http::Request.remote_addr()`.
- Store only the IP string, not the remote port.
- Do not read or trust `X-Forwarded-For`, `X-Real-IP`, or other client-provided headers.
- Historical rows are not backfilled; missing IP displays as `未知`.
- Existing API key totals and existing hourly rollup behavior must remain stable.
- Do not auto-commit. Suggested commit messages are included for manual handoff only.

## File Structure

### Backend Storage

- Modify `crates/core/src/storage/mod.rs`
  - Add `client_ip: Option<String>` to `RequestLog`.
  - Add `client_ip: Option<String>` to `RequestTokenStat`.
  - Add `ClientIpUsageSummary` storage row struct.
- Modify `crates/core/src/storage/request_logs.rs`
  - Add `client_ip` to request log table creation, schema compatibility, insert SQL, select SQL, row mapping, and indexes.
  - Add IP search support through filter builder inputs.
- Modify `crates/core/src/storage/request_log_filters.rs`
  - Include `r.client_ip` in text-search conditions.
- Modify `crates/core/src/storage/request_token_stats.rs`
  - Add `client_ip` to token stats table creation, schema compatibility, insert SQL path, indexes, raw rollup, client-IP rollup table, and client-IP summary query.
- Modify `crates/core/src/storage/tests/request_logs_tests.rs`
  - Cover persistence, listing, and search by IP.
- Modify `crates/core/src/storage/tests/request_token_stats_tests.rs`
  - Cover persistence, grouping by `key_id + client_ip`, exclusion of null/empty IP, key filtering, and rollup retention.

### Backend Gateway And RPC

- Modify `crates/service/src/gateway/request/request_entry.rs`
  - Extract peer IP before validation and pass it into validation/logging paths.
- Modify `crates/service/src/gateway/local_validation/mod.rs`
  - Add `client_ip` field to `LocalValidationResult`.
  - Add parameter to `prepare_local_request`.
- Modify `crates/service/src/gateway/local_validation/request.rs`
  - Copy `client_ip` into all `LocalValidationResult` constructors.
- Modify `crates/service/src/gateway/observability/request_log.rs`
  - Add `client_ip` to `RequestLogTraceContext`.
  - Copy it into `RequestLog` and `RequestTokenStat`.
- Modify `crates/service/src/gateway/observability/tests/request_log_tests.rs`
  - Verify centralized writer persists client IP to both tables.
- Modify `crates/core/src/rpc/types.rs`
  - Add `client_ip` to `RequestLogSummary`.
  - Add `ClientIpUsageSummaryResult`, `ClientIpUsageListParams`, and `ClientIpUsageListResult`.
- Modify `crates/service/src/requestlog/requestlog_list.rs`
  - Copy `client_ip` into `RequestLogSummary`.
- Create `crates/service/src/requestlog/requestlog_client_ip_usage.rs`
  - Normalize params, enforce time range, call storage summaries.
- Modify `crates/service/src/requestlog/mod.rs`
  - Export the new requestlog module.
- Modify `crates/service/src/rpc_dispatch/requestlog.rs`
  - Add `requestlog/client_ip_usage` dispatch with admin/member key filtering.
- Modify `crates/service/src/rpc_dispatch/mod.rs`
  - Add method name to supported RPC method list if the local method registry uses explicit listing.
- Add tests in `crates/service/src/requestlog/requestlog_client_ip_usage_tests.rs` or existing requestlog test modules
  - Cover admin visibility, member key filtering, and serialization.

### Tauri And Web Command Bridge

- Modify `apps/src-tauri/src/commands/requestlog.rs`
  - Add `service_requestlog_client_ip_usage` command.
- Modify `apps/src-tauri/src/commands/registry.rs`
  - Register the new Tauri command.
- Modify `apps/src/lib/api/transport-web-commands/misc.ts`
  - Add `service_requestlog_client_ip_usage: { rpcMethod: "requestlog/client_ip_usage" }`.
- Modify `apps/tests/transport-web-commands.test.mjs`
  - Assert the new command maps to the new RPC method.
- Modify `apps/tests/tauri-command-registry.test.mjs`
  - Assert the command is registered.

### Frontend Types And UI

- Modify `apps/src/types/request-log.ts`
  - Add `clientIp` to `RequestLog`.
  - Add `ClientIpUsageSummary` and `ClientIpUsageListResult`.
- Modify `apps/src/lib/api/normalize.ts`
  - Normalize `clientIp` / `client_ip` on request logs.
  - Add `normalizeClientIpUsageListResult`.
- Modify `apps/src/lib/api/service-client.ts`
  - Add `listClientIpUsage`.
- Modify `apps/src/app/logs/page-sections.tsx`
  - Add `客户端 IP` column.
  - Update empty/skeleton colspans.
  - Update search placeholder to mention IP.
- Modify `apps/src/app/logs/page-cells.tsx`
  - Add reusable `ClientIpCell`.
- Modify `apps/src/app/apikeys/page.tsx`
  - Fetch client-IP usage while page is active and service is ready.
  - Add compact "内网 IP 用量" section near existing usage overview.
  - Show IP, key name/id, requests, success/error, tokens, estimated cost, and last seen.

## Task 1: Storage Schema And Request Log IP Persistence

**Files:**
- Modify: `crates/core/src/storage/mod.rs`
- Modify: `crates/core/src/storage/request_logs.rs`
- Modify: `crates/core/src/storage/request_log_filters.rs`
- Test: `crates/core/src/storage/tests/request_logs_tests.rs`

- [ ] **Step 1: Add failing request-log persistence test**

Append this test to `crates/core/src/storage/tests/request_logs_tests.rs`:

```rust
#[test]
fn request_logs_persist_and_search_client_ip() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    let first_id = storage
        .insert_request_log(&RequestLog {
            trace_id: Some("trace-ip-1".to_string()),
            key_id: Some("key-ip".to_string()),
            client_ip: Some("192.168.1.23".to_string()),
            request_path: "/v1/responses".to_string(),
            method: "POST".to_string(),
            status_code: Some(200),
            created_at: 1_000,
            ..Default::default()
        })
        .expect("insert request log");
    assert!(first_id > 0);

    storage
        .insert_request_log(&RequestLog {
            trace_id: Some("trace-ip-2".to_string()),
            key_id: Some("key-ip".to_string()),
            client_ip: Some("192.168.1.24".to_string()),
            request_path: "/v1/chat/completions".to_string(),
            method: "POST".to_string(),
            status_code: Some(200),
            created_at: 1_001,
            ..Default::default()
        })
        .expect("insert second request log");

    let logs = storage
        .list_request_logs(Some("192.168.1.23"), 10)
        .expect("list request logs by ip");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].trace_id.as_deref(), Some("trace-ip-1"));
    assert_eq!(logs[0].client_ip.as_deref(), Some("192.168.1.23"));
}
```

- [ ] **Step 2: Run failing request-log test**

Run:

```powershell
cargo test -p codexmanager-core request_logs_persist_and_search_client_ip
```

Expected before implementation: compile failure mentioning `RequestLog` has no field `client_ip`, or an assertion failure because IP is not persisted/searched.

- [ ] **Step 3: Add storage fields**

In `crates/core/src/storage/mod.rs`, add `client_ip` to both structs:

```rust
#[derive(Debug, Clone, Default)]
pub struct RequestLog {
    pub trace_id: Option<String>,
    pub key_id: Option<String>,
    pub account_id: Option<String>,
    pub client_ip: Option<String>,
    // keep existing fields below
}

#[derive(Debug, Clone, Default)]
pub struct RequestTokenStat {
    pub request_log_id: i64,
    pub key_id: Option<String>,
    pub account_id: Option<String>,
    pub client_ip: Option<String>,
    // keep existing fields below
}
```

- [ ] **Step 4: Add request log schema, indexes, insert, select, and mapper changes**

In `crates/core/src/storage/request_logs.rs`:

1. Include `r.client_ip` immediately after `r.account_id` in `REQUEST_LOG_LIST_SELECT_COLUMNS`.
2. Add `client_ip TEXT` to each `CREATE TABLE request_logs` definition.
3. Add `self.ensure_column("request_logs", "client_ip", "TEXT")?;` to `ensure_request_logs_table`.
4. Add indexes:

```rust
self.conn.execute(
    "CREATE INDEX IF NOT EXISTS idx_request_logs_client_ip_created_at_id
     ON request_logs(client_ip, created_at DESC, id DESC)",
    [],
)?;
self.conn.execute(
    "CREATE INDEX IF NOT EXISTS idx_request_logs_key_client_ip_created_at_id
     ON request_logs(key_id, client_ip, created_at DESC, id DESC)",
    [],
)?;
```

5. Add `client_ip` to both `INSERT INTO request_logs (...)` statements and params.
6. Update `map_request_log_row` indices by inserting:

```rust
client_ip: row.get(3)?,
```

after `account_id`, then shift later index reads by one.

- [ ] **Step 5: Add IP text search condition**

In `crates/core/src/storage/request_log_filters.rs`, add `r.client_ip` to the query text fields. The condition should match the existing search style and include:

```sql
LOWER(IFNULL(r.client_ip, '')) LIKE ?
```

Use the same normalized `%query%` parameter as path, key, account, and model search.

- [ ] **Step 6: Run request-log tests**

Run:

```powershell
cargo test -p codexmanager-core request_logs
```

Expected after implementation: all request log storage tests pass.

Manual commit suggestion only:

```text
feat: persist client ip in request logs
```

## Task 2: Token Stats IP Persistence And Aggregation

**Files:**
- Modify: `crates/core/src/storage/mod.rs`
- Modify: `crates/core/src/storage/request_token_stats.rs`
- Test: `crates/core/src/storage/tests/request_token_stats_tests.rs`

- [ ] **Step 1: Add failing token-stat grouping test**

Append this test to `crates/core/src/storage/tests/request_token_stats_tests.rs`:

```rust
#[test]
fn summarizes_usage_by_key_and_client_ip() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    for (request_log_id, key_id, client_ip, total_tokens, status_code, error, created_at) in [
        (1_i64, "key-a", Some("192.168.1.23"), 100_i64, Some(200_i64), None, 1_000_i64),
        (2_i64, "key-a", Some("192.168.1.23"), 50_i64, Some(500_i64), Some("upstream failed"), 1_010_i64),
        (3_i64, "key-a", Some("192.168.1.24"), 25_i64, Some(200_i64), None, 1_020_i64),
        (4_i64, "key-b", Some("192.168.1.23"), 200_i64, Some(200_i64), None, 1_030_i64),
        (5_i64, "key-a", None, 999_i64, Some(200_i64), None, 1_040_i64),
    ] {
        storage
            .insert_request_log(&RequestLog {
                trace_id: Some(format!("trace-{request_log_id}")),
                key_id: Some(key_id.to_string()),
                client_ip: client_ip.map(str::to_string),
                request_path: "/v1/responses".to_string(),
                method: "POST".to_string(),
                status_code,
                error: error.map(str::to_string),
                created_at,
                ..Default::default()
            })
            .expect("insert log");
        storage
            .insert_request_token_stat(&RequestTokenStat {
                request_log_id,
                key_id: Some(key_id.to_string()),
                client_ip: client_ip.map(str::to_string),
                total_tokens: Some(total_tokens),
                estimated_cost_usd: Some(total_tokens as f64 / 1000.0),
                created_at,
                ..Default::default()
            })
            .expect("insert stat");
    }

    let rows = storage
        .summarize_request_token_stats_by_key_and_client_ip_between(900, 2_000, None)
        .expect("summarize client ip usage");

    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0].key_id, "key-b");
    assert_eq!(rows[0].client_ip, "192.168.1.23");
    assert_eq!(rows[0].usage.total_tokens, 200);
    assert_eq!(rows[0].usage.success_count, 1);

    let key_a_23 = rows
        .iter()
        .find(|row| row.key_id == "key-a" && row.client_ip == "192.168.1.23")
        .expect("key-a 192.168.1.23 row");
    assert_eq!(key_a_23.usage.request_count, 2);
    assert_eq!(key_a_23.usage.success_count, 1);
    assert_eq!(key_a_23.usage.error_count, 1);
    assert_eq!(key_a_23.usage.total_tokens, 150);
    assert_eq!(key_a_23.last_seen_at, 1_010);
}
```

- [ ] **Step 2: Add failing key-filter test**

Append this test to `crates/core/src/storage/tests/request_token_stats_tests.rs`:

```rust
#[test]
fn client_ip_usage_summary_respects_key_filter() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    for (request_log_id, key_id, client_ip, total_tokens) in [
        (11_i64, "member-key", "192.168.1.50", 40_i64),
        (12_i64, "other-key", "192.168.1.51", 80_i64),
    ] {
        storage
            .insert_request_log(&RequestLog {
                trace_id: Some(format!("trace-filter-{request_log_id}")),
                key_id: Some(key_id.to_string()),
                client_ip: Some(client_ip.to_string()),
                request_path: "/v1/responses".to_string(),
                method: "POST".to_string(),
                status_code: Some(200),
                created_at: request_log_id,
                ..Default::default()
            })
            .expect("insert log");
        storage
            .insert_request_token_stat(&RequestTokenStat {
                request_log_id,
                key_id: Some(key_id.to_string()),
                client_ip: Some(client_ip.to_string()),
                total_tokens: Some(total_tokens),
                created_at: request_log_id,
                ..Default::default()
            })
            .expect("insert stat");
    }

    let rows = storage
        .summarize_request_token_stats_by_key_and_client_ip_between(
            0,
            100,
            Some(&["member-key".to_string()]),
        )
        .expect("summarize filtered client ip usage");

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].key_id, "member-key");
    assert_eq!(rows[0].client_ip, "192.168.1.50");
    assert_eq!(rows[0].usage.total_tokens, 40);
}
```

- [ ] **Step 3: Run failing token-stat tests**

Run:

```powershell
cargo test -p codexmanager-core client_ip_usage_summary
cargo test -p codexmanager-core summarizes_usage_by_key_and_client_ip
```

Expected before implementation: compile failure for missing `ClientIpUsageSummary`, missing `insert_request_token_stat` client IP insert support, or missing summary method.

- [ ] **Step 4: Add storage summary type**

In `crates/core/src/storage/mod.rs`, add this near other token usage summary structs:

```rust
#[derive(Debug, Clone, Default)]
pub struct ClientIpUsageSummary {
    pub key_id: String,
    pub client_ip: String,
    pub usage: TokenUsageRollup,
    pub last_seen_at: i64,
}
```

- [ ] **Step 5: Persist `client_ip` in request token stats**

In `crates/core/src/storage/request_token_stats.rs`:

1. Add `client_ip TEXT` to `CREATE TABLE request_token_stats`.
2. Add `self.ensure_column("request_token_stats", "client_ip", "TEXT")?;`.
3. Add indexes:

```rust
self.conn.execute(
    "CREATE INDEX IF NOT EXISTS idx_request_token_stats_client_ip_created_at
     ON request_token_stats(client_ip, created_at DESC)",
    [],
)?;
self.conn.execute(
    "CREATE INDEX IF NOT EXISTS idx_request_token_stats_key_client_ip_created_at
     ON request_token_stats(key_id, client_ip, created_at DESC)",
    [],
)?;
```

4. Update token-stat insert SQL in `crates/core/src/storage/request_logs.rs` and any standalone `insert_request_token_stat` helper to include `client_ip`.
5. For legacy backfill from `request_logs`, select `client_ip` only if the column exists; otherwise use `NULL`.

- [ ] **Step 6: Add independent client-IP hourly rollup table**

In `ensure_request_token_stats_table`, create the table:

```rust
self.conn.execute(
    "CREATE TABLE IF NOT EXISTS request_token_stat_client_ip_hourly_rollups (
        bucket_start INTEGER NOT NULL,
        bucket_end INTEGER NOT NULL,
        key_id TEXT NOT NULL DEFAULT '',
        client_ip TEXT NOT NULL DEFAULT '',
        input_tokens INTEGER NOT NULL DEFAULT 0,
        cached_input_tokens INTEGER NOT NULL DEFAULT 0,
        output_tokens INTEGER NOT NULL DEFAULT 0,
        total_tokens INTEGER NOT NULL DEFAULT 0,
        reasoning_output_tokens INTEGER NOT NULL DEFAULT 0,
        estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
        request_count INTEGER NOT NULL DEFAULT 0,
        success_count INTEGER NOT NULL DEFAULT 0,
        error_count INTEGER NOT NULL DEFAULT 0,
        last_seen_at INTEGER NOT NULL DEFAULT 0,
        updated_at INTEGER NOT NULL,
        PRIMARY KEY(bucket_start, key_id, client_ip)
     )",
    [],
)?;
self.conn.execute(
    "CREATE INDEX IF NOT EXISTS idx_request_token_stat_client_ip_hourly_key_bucket
     ON request_token_stat_client_ip_hourly_rollups(key_id, bucket_start)",
    [],
)?;
self.conn.execute(
    "CREATE INDEX IF NOT EXISTS idx_request_token_stat_client_ip_hourly_ip_bucket
     ON request_token_stat_client_ip_hourly_rollups(client_ip, bucket_start)",
    [],
)?;
```

- [ ] **Step 7: Roll up client-IP stats before deleting raw rows**

In `rollup_request_token_stats_before`, before `delete_request_token_stats_before_sql()`, add an `INSERT INTO request_token_stat_client_ip_hourly_rollups ... SELECT ... FROM request_token_stats t LEFT JOIN request_logs r ...` that:

- Buckets by `(t.created_at / 3600) * 3600`.
- Uses `COALESCE(NULLIF(TRIM(t.key_id), ''), NULLIF(TRIM(r.key_id), ''))`.
- Uses `COALESCE(NULLIF(TRIM(t.client_ip), ''), NULLIF(TRIM(r.client_ip), ''))`.
- Excludes missing key or missing IP.
- Sums the same token/cost columns used by existing rollups.
- Sets `request_count`, `success_count`, `error_count`, and `last_seen_at`.
- Uses `ON CONFLICT(bucket_start, key_id, client_ip) DO UPDATE` to add metrics and keep the max `last_seen_at`.

- [ ] **Step 8: Add summary method over raw + client-IP hourly rows**

In `crates/core/src/storage/request_token_stats.rs`, add:

```rust
pub fn summarize_request_token_stats_by_key_and_client_ip_between(
    &self,
    start_ts: i64,
    end_ts: i64,
    key_ids: Option<&[String]>,
) -> Result<Vec<ClientIpUsageSummary>> {
    if end_ts <= start_ts {
        return Ok(Vec::new());
    }
    // Use TempKeyIdFilter when key_ids is Some, following existing key filter helpers.
    // Combine raw request_token_stats and request_token_stat_client_ip_hourly_rollups.
    // Group by key_id, client_ip.
    // Order by total_tokens DESC, key_id ASC, client_ip ASC.
}
```

Implement the body with existing helper patterns from `summarize_request_token_stats_by_key_between_with_filter` and `raw_token_rollup_select`. The returned rows must exclude `NULL` and empty `client_ip`.

- [ ] **Step 9: Run token-stat tests**

Run:

```powershell
cargo test -p codexmanager-core request_token_stats
```

Expected after implementation: all token stat storage tests pass, including the new grouping and filter tests.

Manual commit suggestion only:

```text
feat: summarize token usage by client ip
```

## Task 3: Capture Peer IP In Gateway Logs

**Files:**
- Modify: `crates/service/src/gateway/request/request_entry.rs`
- Modify: `crates/service/src/gateway/local_validation/mod.rs`
- Modify: `crates/service/src/gateway/local_validation/request.rs`
- Modify: `crates/service/src/gateway/observability/request_log.rs`
- Test: `crates/service/src/gateway/observability/tests/request_log_tests.rs`

- [ ] **Step 1: Add failing centralized writer test**

Append this test to `crates/service/src/gateway/observability/tests/request_log_tests.rs`:

```rust
#[test]
fn write_request_log_persists_client_ip_to_log_and_token_stat() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");

    super::write_request_log(
        &storage,
        super::RequestLogTraceContext {
            trace_id: Some("trace-client-ip"),
            original_path: Some("/v1/responses"),
            adapted_path: Some("/v1/responses"),
            client_ip: Some("192.168.1.23"),
            ..Default::default()
        },
        Some("key-client-ip"),
        Some("account-client-ip"),
        "/v1/responses",
        "POST",
        Some("gpt-5"),
        None,
        None,
        Some(200),
        super::RequestLogUsage {
            total_tokens: Some(321),
            ..Default::default()
        },
        None,
        Some(12),
    );

    let logs = storage
        .list_request_logs(Some("trace-client-ip"), 10)
        .expect("list logs");
    assert_eq!(logs.len(), 1);
    assert_eq!(logs[0].client_ip.as_deref(), Some("192.168.1.23"));

    let usage = storage
        .summarize_request_token_stats_by_key_and_client_ip_between(0, i64::MAX, None)
        .expect("summarize by ip");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].key_id, "key-client-ip");
    assert_eq!(usage[0].client_ip, "192.168.1.23");
    assert_eq!(usage[0].usage.total_tokens, 321);
}
```

- [ ] **Step 2: Run failing gateway writer test**

Run:

```powershell
cargo test -p codexmanager-service write_request_log_persists_client_ip_to_log_and_token_stat
```

Expected before implementation: compile failure for missing `client_ip` on `RequestLogTraceContext`.

- [ ] **Step 3: Add peer IP normalizer**

In `crates/service/src/gateway/request/request_entry.rs`, add helper functions near `handle_gateway_request`:

```rust
fn normalize_remote_client_ip(request: &Request) -> Option<String> {
    request
        .remote_addr()
        .map(|addr| addr.ip().to_string())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
```

- [ ] **Step 4: Capture IP before validation**

In `handle_gateway_request`, after health/OPTIONS handling and before `prepare_local_request`, add:

```rust
let client_ip_for_log = normalize_remote_client_ip(&request);
```

Pass it into validation:

```rust
super::local_validation::prepare_local_request(
    &mut request,
    trace_id.clone(),
    debug,
    client_ip_for_log.clone(),
)
```

In the validation error logging block, set:

```rust
client_ip: client_ip_for_log.as_deref(),
```

inside `RequestLogTraceContext`.

- [ ] **Step 5: Carry IP through validation result**

In `crates/service/src/gateway/local_validation/mod.rs`:

```rust
pub(super) struct LocalValidationResult {
    pub(super) trace_id: String,
    pub(super) client_ip: Option<String>,
    // keep existing fields
}

pub(super) fn prepare_local_request(
    request: &mut Request,
    trace_id: String,
    debug: bool,
    client_ip: Option<String>,
) -> Result<LocalValidationResult, LocalValidationError> {
    // keep existing body
    request::build_local_validation_result(
        request,
        trace_id,
        incoming_headers,
        storage,
        body,
        api_key,
        client_ip,
    )
}
```

In `crates/service/src/gateway/local_validation/request.rs`, add `client_ip: Option<String>` to `build_local_validation_result` and set `client_ip: client_ip.clone()` or `client_ip` on every `LocalValidationResult` constructor.

- [ ] **Step 6: Add IP to request log trace context and writes**

In `crates/service/src/gateway/observability/request_log.rs`:

```rust
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct RequestLogTraceContext<'a> {
    pub trace_id: Option<&'a str>,
    pub client_ip: Option<&'a str>,
    // keep existing fields
}
```

Normalize the field in `write_request_log_with_attempts`:

```rust
let client_ip = normalize_log_text(trace_context.client_ip);
```

Copy it into both rows:

```rust
client_ip: client_ip.clone(),
```

for `RequestLog`, and:

```rust
client_ip,
```

for `RequestTokenStat`.

- [ ] **Step 7: Pass IP at gateway write call sites**

Search in `crates/service/src/gateway` for `RequestLogTraceContext {` and add:

```rust
client_ip: validated.client_ip.as_deref(),
```

for call sites that receive `LocalValidationResult`. Keep local validation error call sites using `client_ip_for_log.as_deref()`.

- [ ] **Step 8: Run gateway tests**

Run:

```powershell
cargo test -p codexmanager-service request_log
```

Expected after implementation: request log service/gateway tests pass.

Manual commit suggestion only:

```text
feat: capture gateway client ip
```

## Task 4: Requestlog RPC For Client IP Usage

**Files:**
- Modify: `crates/core/src/rpc/types.rs`
- Modify: `crates/service/src/requestlog/requestlog_list.rs`
- Create: `crates/service/src/requestlog/requestlog_client_ip_usage.rs`
- Modify: `crates/service/src/requestlog/mod.rs`
- Modify: `crates/service/src/rpc_dispatch/requestlog.rs`
- Modify: `crates/service/src/rpc_dispatch/mod.rs`
- Test: `crates/service/src/requestlog/requestlog_client_ip_usage_tests.rs`
- Test: `crates/service/src/tests/lib_tests.rs`

- [ ] **Step 1: Add failing requestlog summary test**

Create `crates/service/src/requestlog/requestlog_client_ip_usage_tests.rs`:

```rust
use codexmanager_core::rpc::types::ClientIpUsageListParams;
use codexmanager_core::storage::{RequestLog, RequestTokenStat, Storage};

use super::read_client_ip_usage_with_storage;

fn insert_usage(
    storage: &Storage,
    request_log_id: i64,
    key_id: &str,
    client_ip: &str,
    total_tokens: i64,
    created_at: i64,
) {
    storage
        .insert_request_log(&RequestLog {
            trace_id: Some(format!("trace-client-ip-{request_log_id}")),
            key_id: Some(key_id.to_string()),
            client_ip: Some(client_ip.to_string()),
            request_path: "/v1/responses".to_string(),
            method: "POST".to_string(),
            status_code: Some(200),
            created_at,
            ..Default::default()
        })
        .expect("insert request log");
    storage
        .insert_request_token_stat(&RequestTokenStat {
            request_log_id,
            key_id: Some(key_id.to_string()),
            client_ip: Some(client_ip.to_string()),
            total_tokens: Some(total_tokens),
            estimated_cost_usd: Some(0.01),
            created_at,
            ..Default::default()
        })
        .expect("insert token stat");
}

#[test]
fn client_ip_usage_result_is_sorted_and_filtered() {
    let storage = Storage::open_in_memory().expect("open in memory");
    storage.init().expect("init schema");
    insert_usage(&storage, 1, "key-a", "192.168.1.23", 20, 1_000);
    insert_usage(&storage, 2, "key-b", "192.168.1.24", 80, 1_010);

    let result = read_client_ip_usage_with_storage(
        &storage,
        ClientIpUsageListParams {
            start_ts: Some(900),
            end_ts: Some(2_000),
            limit: Some(10),
        },
        Some(&["key-a".to_string()]),
    )
    .expect("client ip usage");

    assert_eq!(result.items.len(), 1);
    assert_eq!(result.items[0].key_id, "key-a");
    assert_eq!(result.items[0].client_ip, "192.168.1.23");
    assert_eq!(result.items[0].total_tokens, 20);
}
```

- [ ] **Step 2: Add RPC types**

In `crates/core/src/rpc/types.rs`, add `client_ip` to `RequestLogSummary`:

```rust
pub client_ip: Option<String>,
```

Add:

```rust
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientIpUsageSummaryResult {
    pub key_id: String,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ClientIpUsageListParams {
    pub start_ts: Option<i64>,
    pub end_ts: Option<i64>,
    pub limit: Option<i64>,
}

impl Default for ClientIpUsageListParams {
    fn default() -> Self {
        Self {
            start_ts: None,
            end_ts: None,
            limit: Some(100),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientIpUsageListResult {
    pub items: Vec<ClientIpUsageSummaryResult>,
}
```

- [ ] **Step 3: Copy `client_ip` into request log summaries**

In `crates/service/src/requestlog/requestlog_list.rs`, set:

```rust
client_ip: item.client_ip,
```

inside `to_request_log_summary`.

- [ ] **Step 4: Implement requestlog service module**

Create `crates/service/src/requestlog/requestlog_client_ip_usage.rs`:

```rust
use codexmanager_core::rpc::types::{
    ClientIpUsageListParams, ClientIpUsageListResult, ClientIpUsageSummaryResult,
};
use codexmanager_core::storage::{ClientIpUsageSummary, Storage};

use crate::requestlog_list::normalize_time_range;

fn normalize_limit(limit: Option<i64>) -> usize {
    limit.unwrap_or(100).clamp(1, 500) as usize
}

fn to_result(item: ClientIpUsageSummary) -> ClientIpUsageSummaryResult {
    ClientIpUsageSummaryResult {
        key_id: item.key_id,
        client_ip: item.client_ip,
        request_count: item.usage.request_count,
        success_count: item.usage.success_count,
        error_count: item.usage.error_count,
        input_tokens: item.usage.input_tokens,
        cached_input_tokens: item.usage.cached_input_tokens,
        output_tokens: item.usage.output_tokens,
        reasoning_output_tokens: item.usage.reasoning_output_tokens,
        total_tokens: item.usage.total_tokens,
        estimated_cost_usd: item.usage.estimated_cost_usd,
        last_seen_at: item.last_seen_at,
    }
}

pub(crate) fn read_client_ip_usage_with_storage(
    storage: &Storage,
    params: ClientIpUsageListParams,
    key_ids: Option<&[String]>,
) -> Result<ClientIpUsageListResult, String> {
    let (start_ts, end_ts) = normalize_time_range(params.start_ts, params.end_ts);
    let start_ts = start_ts.unwrap_or(0);
    let end_ts = end_ts.unwrap_or(i64::MAX);
    if end_ts <= start_ts {
        return Ok(ClientIpUsageListResult { items: Vec::new() });
    }
    let limit = normalize_limit(params.limit);
    let mut items = storage
        .summarize_request_token_stats_by_key_and_client_ip_between(start_ts, end_ts, key_ids)
        .map_err(|err| format!("summarize client ip usage failed: {err}"))?
        .into_iter()
        .map(to_result)
        .collect::<Vec<_>>();
    items.truncate(limit);
    Ok(ClientIpUsageListResult { items })
}

pub(crate) fn read_client_ip_usage(
    params: ClientIpUsageListParams,
    key_ids: Option<&[String]>,
) -> Result<ClientIpUsageListResult, String> {
    let storage = crate::storage_helpers::open_storage()
        .ok_or_else(|| "open storage failed".to_string())?;
    read_client_ip_usage_with_storage(&storage, params, key_ids)
}

#[cfg(test)]
#[path = "requestlog_client_ip_usage_tests.rs"]
mod tests;
```

If `normalize_time_range` is private, change it from `pub(crate)` in `requestlog_list.rs` or duplicate the small normalization function in the new module.

- [ ] **Step 5: Export module and dispatch RPC**

In `crates/service/src/requestlog/mod.rs`:

```rust
#[path = "requestlog_client_ip_usage.rs"]
pub(crate) mod client_ip_usage;
```

In `crates/service/src/rpc_dispatch/requestlog.rs`, import the module and add:

```rust
"requestlog/client_ip_usage" => {
    let params = req
        .params
        .clone()
        .map(serde_json::from_value::<ClientIpUsageListParams>)
        .transpose()
        .map(|params| params.unwrap_or_default())
        .map_err(|err| format!("invalid requestlog/client_ip_usage params: {err}"));
    super::value_or_error(params.and_then(|params| {
        if actor.is_admin() {
            requestlog_client_ip_usage::read_client_ip_usage(params, None)
        } else {
            let (storage, key_ids) = member_requestlog_scope(actor)?;
            requestlog_client_ip_usage::read_client_ip_usage_with_storage(
                &storage,
                params,
                Some(&key_ids),
            )
        }
    }))
}
```

Use the actual import alias pattern already used by `requestlog_summary` and `requestlog_today_summary`.

- [ ] **Step 6: Add supported-method coverage**

If `crates/service/src/rpc_dispatch/mod.rs` has an allowlist, add:

```rust
"requestlog/client_ip_usage",
```

Add a test in `crates/service/src/tests/lib_tests.rs` beside existing requestlog method tests that sends:

```rust
rpc_request("requestlog/client_ip_usage", serde_json::json!({
    "startTs": 0,
    "endTs": 9999999999,
    "limit": 10
}))
```

and asserts a successful JSON-RPC response for an admin actor.

- [ ] **Step 7: Run requestlog RPC tests**

Run:

```powershell
cargo test -p codexmanager-service requestlog_client_ip_usage
cargo test -p codexmanager-service requestlog
```

Expected after implementation: new module tests and existing requestlog tests pass.

Manual commit suggestion only:

```text
feat: expose client ip usage rpc
```

## Task 5: Tauri And Web Command Bridge

**Files:**
- Modify: `apps/src-tauri/src/commands/requestlog.rs`
- Modify: `apps/src-tauri/src/commands/registry.rs`
- Modify: `apps/src/lib/api/transport-web-commands/misc.ts`
- Test: `apps/tests/transport-web-commands.test.mjs`
- Test: `apps/tests/tauri-command-registry.test.mjs`

- [ ] **Step 1: Add failing frontend runtime tests**

In `apps/tests/transport-web-commands.test.mjs`, add an assertion near existing requestlog command assertions:

```js
assert.equal(
  commandMap.service_requestlog_client_ip_usage.rpcMethod,
  "requestlog/client_ip_usage",
);
```

In `apps/tests/tauri-command-registry.test.mjs`, add:

```js
assert.match(registrySource, /service_requestlog_client_ip_usage/);
```

- [ ] **Step 2: Run failing runtime tests**

Run:

```powershell
pnpm -C apps run test:runtime
```

Expected before implementation: assertion failure for missing web command or Tauri registry command.

- [ ] **Step 3: Add Tauri command**

In `apps/src-tauri/src/commands/requestlog.rs`, add:

```rust
#[tauri::command]
pub async fn service_requestlog_client_ip_usage(
    addr: Option<String>,
    start_ts: Option<i64>,
    end_ts: Option<i64>,
    limit: Option<i64>,
) -> Result<serde_json::Value, String> {
    let params = serde_json::json!({
        "startTs": start_ts,
        "endTs": end_ts,
        "limit": limit
    });
    rpc_call_in_background("requestlog/client_ip_usage", addr, Some(params)).await
}
```

- [ ] **Step 4: Register Tauri command**

In `apps/src-tauri/src/commands/registry.rs`, add under the request log block:

```rust
crate::commands::requestlog::service_requestlog_client_ip_usage,
```

- [ ] **Step 5: Add web command mapping**

In `apps/src/lib/api/transport-web-commands/misc.ts`, add:

```ts
service_requestlog_client_ip_usage: { rpcMethod: "requestlog/client_ip_usage" },
```

- [ ] **Step 6: Run runtime tests**

Run:

```powershell
pnpm -C apps run test:runtime
```

Expected after implementation: runtime tests pass.

Manual commit suggestion only:

```text
feat: bridge client ip usage command
```

## Task 6: Frontend Types, Normalization, And API Client

**Files:**
- Modify: `apps/src/types/request-log.ts`
- Modify: `apps/src/lib/api/normalize.ts`
- Modify: `apps/src/lib/api/service-client.ts`
- Test: `apps/tests/request-logs-duration.spec.ts` or add a focused node test under `apps/tests/`

- [ ] **Step 1: Add failing normalization test**

Create `apps/tests/request-log-client-ip-normalize.test.mjs`:

```js
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const normalizeSource = readFileSync("src/lib/api/normalize.ts", "utf8");
const typesSource = readFileSync("src/types/request-log.ts", "utf8");
const serviceClientSource = readFileSync("src/lib/api/service-client.ts", "utf8");

assert.match(typesSource, /clientIp:\s*string/);
assert.match(typesSource, /interface ClientIpUsageSummary/);
assert.match(normalizeSource, /clientIp:\s*asString\(source\.clientIp\s*\?\?\s*source\.client_ip\)/);
assert.match(normalizeSource, /normalizeClientIpUsageListResult/);
assert.match(serviceClientSource, /listClientIpUsage/);
assert.match(serviceClientSource, /service_requestlog_client_ip_usage/);
```

Add the file to `apps/package.json` `test:runtime` script after `tests/request-utils.test.mjs`:

```json
"tests/request-log-client-ip-normalize.test.mjs"
```

- [ ] **Step 2: Run failing runtime test**

Run:

```powershell
pnpm -C apps run test:runtime
```

Expected before implementation: assertion failure for missing `clientIp`, missing normalizer, or missing API wrapper.

- [ ] **Step 3: Add frontend types**

In `apps/src/types/request-log.ts`, add to `RequestLog`:

```ts
clientIp: string;
```

Add:

```ts
export interface ClientIpUsageSummary {
  keyId: string;
  clientIp: string;
  requestCount: number;
  successCount: number;
  errorCount: number;
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningOutputTokens: number;
  totalTokens: number;
  estimatedCostUsd: number;
  lastSeenAt: number | null;
}

export interface ClientIpUsageListResult {
  items: ClientIpUsageSummary[];
}
```

- [ ] **Step 4: Add normalizers**

In `apps/src/lib/api/normalize.ts`, add `ClientIpUsageListResult` to imports and set:

```ts
clientIp: asString(source.clientIp ?? source.client_ip),
```

inside `normalizeRequestLog`.

Add:

```ts
function normalizeClientIpUsageSummary(item: unknown): ClientIpUsageSummary | null {
  const source = asObject(item);
  const keyId = asString(source.keyId ?? source.key_id);
  const clientIp = asString(source.clientIp ?? source.client_ip);
  if (!keyId || !clientIp) return null;
  return {
    keyId,
    clientIp,
    requestCount: asInteger(source.requestCount ?? source.request_count, 0, 0),
    successCount: asInteger(source.successCount ?? source.success_count, 0, 0),
    errorCount: asInteger(source.errorCount ?? source.error_count, 0, 0),
    inputTokens: asInteger(source.inputTokens ?? source.input_tokens, 0, 0),
    cachedInputTokens: asInteger(source.cachedInputTokens ?? source.cached_input_tokens, 0, 0),
    outputTokens: asInteger(source.outputTokens ?? source.output_tokens, 0, 0),
    reasoningOutputTokens: asInteger(
      source.reasoningOutputTokens ?? source.reasoning_output_tokens,
      0,
      0,
    ),
    totalTokens: asInteger(source.totalTokens ?? source.total_tokens, 0, 0),
    estimatedCostUsd: Math.max(
      0,
      toNullableNumber(source.estimatedCostUsd ?? source.estimated_cost_usd) ?? 0,
    ),
    lastSeenAt: toNullableNumber(source.lastSeenAt ?? source.last_seen_at),
  };
}

export function normalizeClientIpUsageListResult(
  payload: unknown,
): ClientIpUsageListResult {
  const source = asObject(payload);
  const items = asArray(source.items ?? payload)
    .map((item) => normalizeClientIpUsageSummary(item))
    .filter((item): item is ClientIpUsageSummary => Boolean(item));
  return { items };
}
```

- [ ] **Step 5: Add typed API wrapper**

In `apps/src/lib/api/service-client.ts`, import `normalizeClientIpUsageListResult` and `ClientIpUsageListResult`, then add:

```ts
async listClientIpUsage(params?: {
  startTs?: number | null;
  endTs?: number | null;
  limit?: number | null;
}, options?: RequestOptions): Promise<ClientIpUsageListResult> {
  const result = await invoke<unknown>(
    "service_requestlog_client_ip_usage",
    withAddr({
      startTs: params?.startTs ?? null,
      endTs: params?.endTs ?? null,
      limit: params?.limit ?? 100,
    }),
    options,
  );
  return normalizeClientIpUsageListResult(result);
},
```

- [ ] **Step 6: Run runtime tests**

Run:

```powershell
pnpm -C apps run test:runtime
```

Expected after implementation: runtime tests pass.

Manual commit suggestion only:

```text
feat: add client ip usage frontend api
```

## Task 7: Logs Page IP Column

**Files:**
- Modify: `apps/src/app/logs/page-sections.tsx`
- Modify: `apps/src/app/logs/page-cells.tsx`
- Test: `apps/tests/request-logs-duration.spec.ts`

- [ ] **Step 1: Add failing UI source test**

Extend `apps/tests/request-logs-duration.spec.ts` or add a source-level node assertion that checks:

```js
assert.match(pageSectionsSource, /客户端 IP/);
assert.match(pageSectionsSource, /ClientIpCell/);
assert.match(pageCellsSource, /function ClientIpCell/);
```

- [ ] **Step 2: Run failing UI test**

Run:

```powershell
pnpm -C apps run test:runtime
```

Expected before implementation: assertion failure for missing IP column/cell.

- [ ] **Step 3: Add `ClientIpCell`**

In `apps/src/app/logs/page-cells.tsx`, add:

```tsx
export function ClientIpCell({ clientIp }: { clientIp: string }) {
  const { t } = useI18n();
  const displayIp = String(clientIp || "").trim() || t("未知");
  return (
    <Tooltip>
      <TooltipTrigger render={<div />} className="block text-left">
        <span className="block max-w-[132px] truncate font-mono text-[11px] text-muted-foreground">
          {displayIp}
        </span>
      </TooltipTrigger>
      <TooltipContent className={`${logTooltipContentClassName} max-w-sm`}>
        <div className="space-y-0.5">
          <div className={logTooltipLabelClassName}>{t("客户端 IP")}</div>
          <div className="break-all font-mono text-[11px]">{displayIp}</div>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
```

- [ ] **Step 4: Add table column**

In `apps/src/app/logs/page-sections.tsx`:

1. Add `ClientIpCell` to imports from `./page-cells`.
2. Change the search placeholder to:

```tsx
placeholder={t("搜索路径、账号、密钥 ID 或客户端 IP...")}
```

3. Increase table width from `min-w-[1500px]` to `min-w-[1620px]`.
4. Add header after `账号 / 密钥`:

```tsx
<TableHead className="w-[140px] px-4 text-[11px] font-semibold tracking-[0.12em] text-muted-foreground uppercase">
  {t("客户端 IP")}
</TableHead>
```

5. Add skeleton cell:

```tsx
<TableCell><Skeleton className="h-4 w-24" /></TableCell>
```

6. Change empty `colSpan` from `8` to `9`.
7. Add row cell after `AccountKeyInfoCell`:

```tsx
<TableCell className="px-4 py-3 align-top">
  <ClientIpCell clientIp={log.clientIp} />
</TableCell>
```

- [ ] **Step 5: Run runtime/build validation**

Run:

```powershell
pnpm -C apps run test:runtime
pnpm -C apps run build:desktop
```

Expected after implementation: runtime tests and static desktop build pass.

Manual commit suggestion only:

```text
feat: show client ip in request logs
```

## Task 8: API Key Page Client-IP Usage Section

**Files:**
- Modify: `apps/src/app/apikeys/page.tsx`
- Test: `apps/tests/request-log-client-ip-normalize.test.mjs` or new `apps/tests/apikey-client-ip-usage.test.mjs`

- [ ] **Step 1: Add failing source test**

Create `apps/tests/apikey-client-ip-usage.test.mjs`:

```js
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync("src/app/apikeys/page.tsx", "utf8");

assert.match(source, /listClientIpUsage/);
assert.match(source, /内网 IP 用量/);
assert.match(source, /clientIpUsage/);
assert.match(source, /lastSeenAt/);
```

Add the file to the `test:runtime` script in `apps/package.json`.

- [ ] **Step 2: Run failing source test**

Run:

```powershell
pnpm -C apps run test:runtime
```

Expected before implementation: assertion failure for missing query/section.

- [ ] **Step 3: Import service client and table helpers**

In `apps/src/app/apikeys/page.tsx`, add:

```ts
import { serviceClient } from "@/lib/api/service-client";
```

Keep existing table imports; no new UI package is needed.

- [ ] **Step 4: Add query**

Inside `ApiKeysPage`, after the existing `usageOverview` query, add:

```tsx
const { data: clientIpUsage, isPending: isClientIpUsageLoading } = useQuery({
  queryKey: ["requestlog", "client-ip-usage", serviceAddr || null],
  queryFn: () => serviceClient.listClientIpUsage({ limit: 100 }),
  enabled: isUsageQueryEnabled && isPageActive,
  retry: 1,
});

const clientIpUsageItems = clientIpUsage?.items || [];
const apiKeyNameById = useMemo(
  () => new Map(apiKeys.map((item) => [item.id, item.name || item.id])),
  [apiKeys],
);
```

- [ ] **Step 5: Add compact section**

Insert this `WorkPanel` between overview metric cards and the existing key table:

```tsx
<WorkPanel>
  <CardContent className="space-y-3 p-4">
    <div className="flex flex-col gap-1 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <div className="text-sm font-semibold">{t("内网 IP 用量")}</div>
        <div className="text-xs text-muted-foreground">
          {t("按平台密钥和直接访问的内网 IP 汇总")}
        </div>
      </div>
      <Badge variant="secondary" className="w-fit rounded-md px-2.5">
        {t("按 Token 排序")}
      </Badge>
    </div>
    <div className="overflow-x-auto">
      <Table className="min-w-[920px]">
        <TableHeader>
          <TableRow>
            <TableHead>{t("客户端 IP")}</TableHead>
            <TableHead>{t("密钥")}</TableHead>
            <TableHead>{t("请求")}</TableHead>
            <TableHead>{t("成功 / 异常")}</TableHead>
            <TableHead>{t("Token")}</TableHead>
            <TableHead>{t("费用")}</TableHead>
            <TableHead>{t("最近出现")}</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {isClientIpUsageLoading ? (
            Array.from({ length: 3 }).map((_, index) => (
              <TableRow key={index}>
                <TableCell><Skeleton className="h-4 w-28" /></TableCell>
                <TableCell><Skeleton className="h-4 w-32" /></TableCell>
                <TableCell><Skeleton className="h-4 w-16" /></TableCell>
                <TableCell><Skeleton className="h-4 w-20" /></TableCell>
                <TableCell><Skeleton className="h-4 w-20" /></TableCell>
                <TableCell><Skeleton className="h-4 w-16" /></TableCell>
                <TableCell><Skeleton className="h-4 w-24" /></TableCell>
              </TableRow>
            ))
          ) : clientIpUsageItems.length === 0 ? (
            <TableRow>
              <TableCell colSpan={7} className="h-28 text-center text-sm text-muted-foreground">
                {t("暂无内网 IP 用量记录")}
              </TableCell>
            </TableRow>
          ) : (
            clientIpUsageItems.map((item) => (
              <TableRow key={`${item.keyId}:${item.clientIp}`}>
                <TableCell>
                  <code className="inline-block max-w-[180px] truncate rounded border border-border/60 bg-muted/50 px-2 py-1 font-mono text-[11px]" title={item.clientIp}>
                    {item.clientIp || t("未知")}
                  </code>
                </TableCell>
                <TableCell>
                  <div className="max-w-[220px] truncate text-sm font-medium" title={apiKeyNameById.get(item.keyId) || item.keyId}>
                    {apiKeyNameById.get(item.keyId) || item.keyId}
                  </div>
                  <div className="max-w-[220px] truncate font-mono text-[10px] text-muted-foreground">
                    {item.keyId}
                  </div>
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {item.requestCount.toLocaleString("zh-CN")}
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {item.successCount.toLocaleString("zh-CN")} / {item.errorCount.toLocaleString("zh-CN")}
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {formatCompactTokenAmount(item.totalTokens)}
                </TableCell>
                <TableCell className="font-mono text-xs">
                  {formatUsd(item.estimatedCostUsd)}
                </TableCell>
                <TableCell className="text-xs text-muted-foreground">
                  {formatLocalMinuteFromSeconds(item.lastSeenAt, t("未知"))}
                </TableCell>
              </TableRow>
            ))
          )}
        </TableBody>
      </Table>
    </div>
  </CardContent>
</WorkPanel>
```

- [ ] **Step 6: Run frontend checks**

Run:

```powershell
pnpm -C apps run test:runtime
pnpm -C apps run build:desktop
```

Expected after implementation: runtime tests and static desktop build pass.

Manual commit suggestion only:

```text
feat: add api key client ip usage view
```

## Task 9: Final Validation

**Files:**
- No new files expected.
- Check all files touched in Tasks 1-8.

- [ ] **Step 1: Run targeted Rust storage tests**

Run:

```powershell
cargo test -p codexmanager-core request_logs
cargo test -p codexmanager-core request_token_stats
```

Expected: both commands pass.

- [ ] **Step 2: Run targeted Rust service tests**

Run:

```powershell
cargo test -p codexmanager-service requestlog
cargo test -p codexmanager-service request_log
```

Expected: both commands pass.

- [ ] **Step 3: Run frontend runtime tests**

Run:

```powershell
pnpm -C apps run test:runtime
```

Expected: runtime tests pass, including Tauri registry, Web command mapping, and source-level IP usage assertions.

- [ ] **Step 4: Run desktop static export build**

Run:

```powershell
pnpm -C apps run build:desktop
```

Expected: Next.js static export build completes successfully.

- [ ] **Step 5: Do not start service for manual LAN validation**

Because the user is already running the app locally, do not run `pnpm dev`, `pnpm dev:desktop`, `cargo run`, or Tauri startup commands. Manual validation can be done by the user after installing the build:

1. Use the same platform API key from two LAN clients that directly call the service.
2. Open request logs and confirm new rows show two different `客户端 IP` values.
3. Open API key page and confirm `内网 IP 用量` has separate rows for the same key and different IPs.
4. Confirm existing total API key token usage still matches the sum from previous key usage view.

## Self-Review Checklist

- Spec coverage:
  - Direct TCP peer IP captured: Task 3.
  - No proxy header trust: Task 3 only uses `remote_addr()`.
  - Request logs show IP: Tasks 1, 4, 6, 7.
  - Usage grouped by `key_id + client_ip`: Task 2.
  - Member permissions: Task 4 uses existing member key scope.
  - Mac desktop compatibility: Tasks 5-9 keep Tauri/static export paths synchronized without platform-specific APIs.
  - No local app startup: Constraints and Task 9.
- Placeholder scan:
  - No deferred placeholder entries are used.
  - Each task has specific files, code snippets, commands, and expected results.
- Type consistency:
  - Rust storage field: `client_ip`.
  - Rust RPC camelCase output: `clientIp`.
  - TypeScript field: `clientIp`.
  - RPC method: `requestlog/client_ip_usage`.
  - Tauri command: `service_requestlog_client_ip_usage`.
