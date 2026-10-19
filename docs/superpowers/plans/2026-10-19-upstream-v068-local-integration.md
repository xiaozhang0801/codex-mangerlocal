# Upstream v0.6.8 Local Integration Plan

**Goal:** Port upstream changes after v0.6.4 through 430eddb0 while preserving CodexManagerLocal behavior.

**Architecture:** Apply bounded upstream dependency chains to the existing checkout, preserving local changes through explicit conflict resolution. Integrate local IP/account settings and personnel maps into upstream's prepare/transaction/publish settings boundary. Reuse upstream protocol, retry, authentication, pricing and packaging regressions.

**Tech Stack:** Rust workspace, SQLite/SeaORM, Tauri v2, Next.js, pnpm, Node runtime tests, Playwright.

**Spec:** Approved chat scope on 2026-10-19. Native execution in current checkout; no worktree, commit, push or release. No sponsor updates. Preserve local brand/database/update URLs and IP names, real-time identity/status, usage/cost/week/sort and configurable single-IP/multi-IP/account concurrency.

## todo_list

- [x] Add upstream/local regression tests first and record failing evidence.
- [x] Port gateway protocols, deadline/failover, safe transport retries, SQLite and authentication dependencies.
- [x] Adapt atomic settings to preserve local clientIpNames, account/IP concurrency and authorization boundaries.
- [x] Port final Codex profile auth semantics, official pricing and applicable release repairs; retain local identity and sponsor content.
- [x] Run workspace/service tests, frontend runtime tests, desktop export, Playwright, formatting and source audits.
- [x] Launch native desktop app with an isolated test database, verify process/window and service, document limits and stop test processes.

## Task Boundaries

1. Regression tests: upstream tests under crates/service/tests/app_settings_atomic.rs, gateway_logs, apps/tests/request-utils.test.mjs and rpc-http.test.mjs; local extensions in crates/service/tests/app_settings/client_ip_names.rs. Require failed batches to preserve persisted/runtime concurrency and IP mappings.
2. Shared behavior: crates/core/storage, crates/rusqlite, crates/storage-seaorm; service gateway observability/routing/upstream, rpc_dispatch and auth; apps transport and src-tauri rpc_client. Reuse upstream API signatures and tests; preserve local admission/activity tracking.
3. Settings: prepare validated local fields into the same BTreeMap change plan as upstream settings, persist in one transaction, then publish runtime gates. Personnel values remain local-only/admin-only, absent fields stay unchanged, empty map clears.
4. Codex and pricing: final 430eddb0 behavior, not intermediate ChatGPT-only prerequisite. Keep auth.json on gateway/aggregate application; direct account mode still writes selected credentials. Preserve exact local brand/updater/release workflow identity.
5. Validation: cargo fmt --all -- --check; cargo test --workspace; pnpm -C apps run test:runtime; pnpm -C apps exec playwright test tests/desktop-request-identity.spec.ts tests/gateway-auth-cleanup.spec.ts (includes build:desktop). Native Tauri build/run uses temporary storage and loopback-only service, never personnel/user production data.

## Review Focus

- Invalid mixed settings or storage failures must not partially apply personnel/concurrency settings.
- Reads and identical settings must not reset round-robin cursors or live IP/account gates.
- Stream completion/failure and tools must preserve current local delivery/activity behavior.
- Retry rules must not replay already-sent non-idempotent writes; uncertain results are explicit.
- No real personnel data, sponsor edits, original-brand database names or upstream update URLs introduced.

## Execution Ledger

- Base: HEAD 62c033d5; upstream range 596d9761..430eddb0, 12 commits. Existing dirty .gitignore and two 2026-10-12 docs remain untouched.
- Ruling: native implementation with optional read-only subagent audit; no repeated approval required for the already-approved scope.
- Ruling: mechanical upstream patches may be applied with git apply after scope inspection; manual conflict and local adaptation edits use apply_patch. No commits or pushes.
- RED: request-utils tests reproduced missing cancellation/body deadlines before the upstream production patch; after migration all 20 pass.
- RED: mixed local concurrency patch plus sseKeepaliveIntervalMs=0 partially persisted settings before migration; the migrated transaction passes this regression.
- RED: environment change reset local account concurrency 7 -> 3. Final fix removes reload resets for these three settings-only runtime cells; save/restore was rejected because it exposed transient defaults to request threads.
- Frontend runtime: 354 passed. Desktop identity/auth Playwright: 7 passed, including desktop export, Edge, 1440/640px, personnel import/rollback and complete single-line IP labels.
- Ruling: keep the current local release version until a separate release request; dependency changes and AppImage fixes are included without overwriting local names, identifiers, updater endpoints or release filters.
- Ruling: retain the local parsed Responses terminal tracker and regressions; adapt the upstream bounded frame pump and add Anthropic terminal tracking rather than reverting to text-marker matching.
- Ruling: removed only formatting-only changes introduced by this run in unrelated Tauri files; workspace formatting and touched-file checks remain required.
- Full Rust/native validation completed. Initial workspace compilation identified missing imports/initializer in retained local stream tests; those test adaptations are corrected.
- RED/GREEN: valid unterminated terminal JSON was claimed as delivered without a dispatched client event. The frame reader now appends the missing SSE delimiter before claiming delivery; regression passed.
- RED/GREEN: environment reload replaced active admission gates and reset configured limits 1/1/1 to 3/3/2. Reload now retains referenced gates and leaves settings-only limits unchanged; both regressions passed.
- Native QA: isolated database/profile/CODEX_HOME and identifier; service 48879, CDP 9226. Actual Tauri IPC settings save, invalid-patch rollback, service start and active-request command passed again on the final rebuilt binary; dashboard and settings screenshots inspected. QA processes and ports 3005/48879/9226 were stopped; the installed user app PID 40720 on 48760 remains running.
- Audit: all changed upstream paths under apps/crates/scripts/.github exist locally and differ from the old upstream baseline. Root/localized README sponsor content and local identity/updater configuration remain unchanged. Two independent read-only reviews checked stream and settings/RPC/storage dependencies.
- Known pre-existing risk outside this port: SQLite backup retries BUSY/LOCKED without an overall deadline under sustained external lock contention. No claim of production account/load testing or macOS installer verification.
- Validation correction: upstream control-line tests were adapted to dispatched-event semantics (nonempty data and resolution of a trailing CR). All nine terminal-delivery tests pass. A Windows LNK1104 retry was required because the previous workspace run still held its rpc test executable; final workspace validation runs after that process exited.

## Final Validation

- `cargo test --workspace --no-fail-fast`: exit 0, 2886 passed, 0 failed, 19 ignored. Includes service library 2002, gateway integration 75, app settings 46, atomic settings 2, RPC 50 and web shell 42 passing tests. Log: `.codex-workspace-final.log`.
- `cargo test --manifest-path apps/src-tauri/Cargo.toml --lib`: exit 0, 96 passed. Log: `.codex-tauri-final.log`.
- `pnpm -C apps run test:runtime`: 354 passed. Desktop export and Playwright identity/auth tests: 7 passed. Logs: `.codex-runtime.log`, `.codex-ui-test.log`.
- AppImage portability/permissions tests under WSL: 20 passed. Log: `.codex-appimage-tests.log`.
- Native custom-protocol build: exit 0; actual app IPC and rendered pages passed using the isolated fixture. Evidence: `.codex-native-qa-build.log`, `.codex-manager-run/upstream-v068/native-dashboard.png`, `native-settings.png`, and `probe.mjs`.
- `cargo fmt --all -- --check` and `git diff --check`: exit 0. Existing Rust unused/dead-code warnings and Git CRLF notices remain; this is not a warning-free claim.
- Prevention: retain the new terminal-dispatch, active-gate and settings-only reload regressions; tests that change concurrency explicitly restore the previous value rather than relying on environment reload.
- Delivery boundary: current checkout only, no commit/push/release, local version remains 0.6.4. Native Windows smoke tests do not prove macOS installer behavior, real upstream credentials or production concurrency capacity.
