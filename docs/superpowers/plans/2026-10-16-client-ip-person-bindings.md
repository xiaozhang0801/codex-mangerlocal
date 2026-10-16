# Client IP Person Bindings Implementation Plan

**Goal:** Import a local personnel XLSX and display IP plus person in dashboard requests and logs without restricting unbound clients.

**Architecture:** Extend the existing app settings get/set chain with `clientIpNames: Record<string, string>`, persisted as one validated JSON setting. Read XLSX in the desktop frontend and preview before replacing the mapping. Share one display component backed by the settings store. No changes to gateway admission, quotas, logs or routing data.

**Tech Stack:** Existing Rust settings storage, typed Tauri transport, React/Zustand, read-excel-file, ipaddr.js, Node tests and Playwright.

**Spec:** User-approved chat design: IP(name), unmatched IP(unbound), full single-line text with horizontal scrolling, local import under settings, existing logs use the current mapping.

## Constraints

- Existing checkout only; no commit/push/package without a new request.
- Personnel workbook and extracted actual identities must not be committed or bundled. Use fictional documentation-range IPs in tests.
- Preserve existing dirty files and unrelated previous plans.
- Use `clientIpNames` on the settings wire; default is `{}`. Replacing with `{}` clears bindings.
- Normalize IPv4/IPv6, including IPv4-mapped IPv6, for lookup without changing displayed original IP.
- Maximum 5000 bindings, names 1-100 characters without control characters; reject invalid IPs and conflicting canonical keys before saving.
- XLSX first sheet: find `姓名` and `IPv4 地址(IP)` / `IP` columns after optional title rows. Skip incomplete rows, reject malformed IPs and conflicting duplicates. Preview and explicit replace confirmation. File limit 2 MiB.
- Do not alter sponsor assets, release identity, database identity or gateway concurrency settings.

## todo_list

- [x] Add Rust settings roundtrip/default/validation tests (RED), implement JSON persistence and settings response/patch wiring (GREEN).
- [x] Add frontend parsing, normalization and label tests (RED); implement helpers, lazy XLSX reader and typed settings defaults (GREEN).
- [x] Add desktop-only settings import/preview/replace/clear UI and shared nowrap IP display at all three requested surfaces.
- [x] Add desktop IPC fixture UI tests for import, persistence reload, unbound clients, full IP labels and failed-save preservation.
- [x] Run relevant Rust tests, runtime tests, desktop build, Playwright, formatting and diff checks. Review security/privacy and report validation limits.

## Review Focus

- No personal data in repository artifacts or diagnostic logs.
- Failed import/save must preserve the previous mapping; parsing another file cannot silently overwrite a newer preview.
- Empty and incomplete workbooks cannot silently clear mappings; clearing requires an explicit separate action.
- Import has no effect on account/IP concurrency and no name becomes a database key for usage aggregation.
- Missing client IP remains unknown rather than a synthetic address; HTML-like names render as text.

## Validation Evidence

- Backend RED: 7 settings integration tests failed before implementation; GREEN: 7 passed, full app_settings suite 43 passed.
- Frontend RED: 6 missing helper/display assertions failed; settings import entry test failed before UI implementation. GREEN: 8 focused tests passed, including real synthetic XLSX bytes.
- Desktop UI first attempt could not load an ESM fixture through Playwright's CJS transform. A shared CJS fixture corrected the test harness; 4 UI tests then passed, with desktop build.
- Full runtime suite initially failed i18n coverage; translations added for en/ko/ru. A follow-up edge test reproduced frontend accepting a mapped IPv4 with octal notation; validation aligned with Rust and control-character checks.
- Production reader uses the worker-free universal export to respect the existing desktop CSP, with a 2 MiB file limit. This is a local import, not a throughput benchmark.
- Personnel workbook is excluded in local .git/info/exclude, not bundled or committed. All test workbook bytes are generated from fictional data.
- Latest frontend runtime suite: 284 passed. Desktop static export and Playwright: 4 passed at 1440px/640px, covering full single-line labels, import, failed-save preservation, reload and clear. Changed frontend files pass ESLint.
- Latest settings integration suite: 43 passed. Privacy-focused service unit tests: 2 passed. Cargo formatting and whitespace diff checks pass.
- Production XLSX reader checked the supplied workbook read-only: 60 bindings and 3 incomplete rows skipped. No real names or IPs were logged or embedded.
- Manual review confirmed no gateway, Tauri release identity or GitHub workflow changes. Independent reviewer agents could not run because the selected model was at capacity; no model substitution was made.
- Expanded service unit regression: `cargo test -p codexmanager-service --lib tests:: -- --test-threads=1` passed with 1879 passed, 8 ignored, 0 failed. Full workspace tests were not run; validation covers the changed service package and desktop frontend. UI tests mock desktop IPC; no native Tauri/Mac installer or live upstream traffic was exercised. No commit, push or package was performed.

## Regression Lessons

- Generic app settings are readable by members: adding private data without filtering would expose the personnel directory. Added admin-only map writes and removed the map from non-admin responses, with a reproduced failing permission test followed by a passing regression.
- Generic settings can use a remote storage backend: reject personnel writes and omit stored names in that mode so this local feature cannot silently become a remote directory. Added a backend-mode regression test without connecting to a remote database.
- Excel library defaults trim cell values before validation, hiding leading control characters. Disabled library trimming and retained explicit name validation/ordinary whitespace cleanup. A generated XLSX first reproduced the missing rejection, then passed after the fix.
- Keep importer tests at both row-helper and actual XLSX-byte levels, and test permission boundaries whenever shared settings gain private fields.
- Expanded service regression initially had 1878 passes, 8 ignored and one failure in `member_actor_cannot_call_admin_only_rpc`; that test passed alone. It lacked an environment guard/database setup and inherited the preceding password-mode test's settings. Added a guarded independent database and explicit accounts mode to the test, without changing production authorization. Full service unit rerun passed, including this previously failing test.
