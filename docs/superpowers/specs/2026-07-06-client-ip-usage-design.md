# Client IP Usage Monitoring Design

## Background

The app already records gateway request logs and token usage by API key, account,
model, and upstream source. It does not currently record the LAN client that made
the request. When multiple LAN machines use the same API key, their usage is
merged and cannot be separated.

The chosen scope is option 2: record client IP in both request logs and token
usage stats, then expose key-plus-IP usage summaries in the UI.

## Goals

- Record the direct TCP peer IP for every gateway request where it is available.
- Show the client IP on request logs.
- Aggregate usage by `key_id + client_ip`, including request count, success
  count, error count, tokens, estimated cost, and last seen time.
- Preserve existing direct desktop, service-mode web, and Mac desktop behavior.
- Avoid trusting spoofable proxy headers by default.

## Non-Goals

- Do not identify a person or device beyond the observed IP address.
- Do not support `X-Forwarded-For` or `X-Real-IP` in this first pass.
- Do not backfill historical logs; older rows will have no client IP.
- Do not start or stop the user's local app during implementation.

## IP Source

The gateway uses `tiny_http::Request`, which exposes the TCP peer address through
`request.remote_addr()`. The implementation will normalize that value to an IP
string without a port:

- LAN clients will appear as values like `192.168.1.23`.
- Local clients will appear as `127.0.0.1` or `::1`.
- If a request path cannot provide a peer address, the field stays `NULL`.

Because the user confirmed all clients directly access the service, proxy header
support is intentionally omitted. This avoids letting a client forge another
machine's IP through request headers.

## Data Model

Add nullable `client_ip TEXT` to:

- `request_logs`
- `request_token_stats`

Add indexes for common lookups:

- `request_logs(client_ip, created_at DESC, id DESC)`
- `request_logs(key_id, client_ip, created_at DESC, id DESC)`
- `request_token_stats(client_ip, created_at)`
- `request_token_stats(key_id, client_ip, created_at)`

For long-term summaries beyond raw stat retention, add a separate rollup table
instead of changing the existing hourly rollup primary key:

`request_token_stat_client_ip_hourly_rollups`

Dimensions:

- `bucket_start`
- `bucket_end`
- `key_id`
- `client_ip`

Metrics:

- `input_tokens`
- `cached_input_tokens`
- `output_tokens`
- `reasoning_output_tokens`
- `total_tokens`
- `estimated_cost_usd`
- `request_count`
- `success_count`
- `error_count`
- `last_seen_at`
- `updated_at`

This keeps existing rollup behavior stable and limits migration risk.

## Backend Flow

1. Extract `client_ip` near `handle_gateway_request` before validation consumes
   or moves the request.
2. Pass `client_ip` into `prepare_local_request`, store it on
   `LocalValidationResult`, and copy it into `RequestLogTraceContext` at every
   request-log write site. `RequestLogTraceContext` then becomes the single
   writer-facing carrier for the value.
3. In `write_request_log_with_attempts`, copy `client_ip` into both
   `RequestLog` and `RequestTokenStat`.
4. Ensure validation errors and locally handled gateway responses also carry the
   same `client_ip` into logs.
5. Add storage query methods for client-IP usage summaries between a time range,
   optionally filtered by key IDs for non-admin users. Summary rows exclude
   `NULL` or empty `client_ip`; those legacy or unavailable records remain
   visible in request logs as `未知`.
6. Add an RPC method for the UI, for example
   `requestlog/client_ip_usage`.

## Frontend Flow

Update request log types and normalization:

- Add `clientIp` to `RequestLog`.
- Add `ClientIpUsageSummary` type for aggregated usage rows.
- Add a typed API wrapper in `apps/src/lib/api/service-client.ts`.

UI placement:

- Logs page: add a narrow "客户端 IP" table column and make the existing search
  able to match IP values.
- API key page: add a dense "内网 IP 用量" section near the existing key usage
  overview, sorted by total tokens by default. Each row shows IP, key name/id,
  requests, success/error, total tokens, estimated cost, and last seen time.

The UI should display empty IPs as `未知` and avoid layout expansion on long IPv6
values by using monospace text with truncation and tooltip details.

## Permissions

Admin users can see all key-plus-IP usage rows.

Member users can only see rows for API keys they own, matching the current
request log scope rules.

## Testing

Rust storage tests:

- Inserting request logs and token stats persists `client_ip`.
- Listing logs returns `client_ip`.
- Summary query groups by `key_id + client_ip`.
- Empty or missing IP groups as unknown or is excluded according to the API
  contract.
- Member key filtering does not leak other users' IP usage.

Rust service tests:

- Request log RPC serializes `clientIp`.
- Client-IP usage RPC returns expected aggregates and handles empty ranges.

Frontend tests or build checks:

- TypeScript normalization maps `client_ip` and `clientIp`.
- Request log UI handles missing IP.
- Desktop static export build remains valid.

Validation commands after implementation:

- `cargo test -p codexmanager-core request_token_stats`
- `cargo test -p codexmanager-core request_logs`
- `cargo test -p codexmanager-service requestlog`
- `pnpm -C apps run build:desktop`

If a command cannot run in the local environment, the final report must include
the exact command and failure reason.

## Risks

- IP addresses can change if devices use DHCP. The feature tracks observed IPs,
  not permanent devices.
- If a client still connects through localhost or a local proxy, all usage will
  appear under `127.0.0.1` or `::1`.
- Historical rows before this change will not have IP attribution.
- IP addresses are local operational metadata, so they should be displayed only
  in admin/member scopes that already have request-log access.

## Acceptance Criteria

- New gateway requests record a client IP when directly accessed from LAN.
- The same API key used by two LAN IPs produces two separate usage summary rows.
- Logs show the client IP for new requests.
- Existing API key usage totals remain unchanged.
- Existing request log pages and member permissions continue to work.
