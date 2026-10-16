# Local Upstream and Request Identity Implementation Plan

**Goal:** Include all missing non-sponsor upstream changes through 596d9761 while preserving CodexManagerLocal behavior; display active-request accounts and teams and full single-line client IPs.

**Architecture:** Port missing upstream changes individually. Capture selected account metadata in the existing in-memory request activity record, without additional per-request database queries. Preserve the source fields for compatibility but remove their visible column. Keep the current account and IP concurrency limits.

**Tech Stack:** Rust, existing typed RPC/Tauri transport, Next.js/React, Node tests, Playwright.

## Constraints

- Work in the existing checkout. Preserve pre-existing changes to .gitignore and apps/src-tauri/Cargo.toml.
- Exclude sponsor README, sponsor assets and sponsor link updates.
- Preserve local app/database identity, updater URLs, local release workflows, IP usage/cost/week summaries, sorting and concurrency settings.
- Team display uses the existing account group_name, with workspace_id as fallback; no group-based concurrency is added.
- Unassigned requests display pending assignment; aggregate requests must not inherit a previous account identity.
- Full client IPs remain on one line. Use horizontal table scrolling, never truncation or wrapping.
- No commit, push or GitHub build is requested in this turn.

## todo_list

- [x] Audit all upstream-only commits against adapted local commits; record sponsor exclusions and missing work.
- [x] Add upstream #502/#504 regression tests, observe failures, port implementations and verify.
- [x] Add active-request identity and account queue state/timing tests, observe failures, implement metadata and state transitions.
- [x] Add frontend identity normalization/rendering and full-IP coverage; replace source column with account/team columns.
- [x] Synchronize v0.6.4 metadata without changing local identity or release configuration; document integration coverage.
- [x] Run Rust workspace tests, frontend runtime tests and desktop build, then desktop-transport UI tests at narrow/wide viewports.
- [x] Review changes and report validation limits.

## Review Focus

- Account retries must update displayed account/team and preserve accumulated queue timing.
- Waiting for account capacity must remain queued, and completed streams must release activity and concurrency guards.
- Personal accounts, missing labels/groups and aggregate routes must have honest fallback labels.
- Long IPv6 addresses must remain fully readable in horizontally scrollable tables.
- Upstream errors must remain failures, and historical search repair must not enable unwanted new tool calls.

## Upstream Coverage

Comparison: local HEAD `0b3ab34c` against fetched `origin/main` `596d9761` (2026-10-15).
Of 34 upstream-only commits, 12 sponsor-only commits are excluded. The other 22
consist of 19 already adapted changes, two missing fixes, and v0.6.4 metadata.

| Upstream | Existing local equivalent / action |
| --- | --- |
| #481 e2ed4ba4 | 8693a439 |
| Navigation/settings 420274ef | 456ef727 |
| v0.6.3 76453142 | 1932d96b, local branding preserved |
| Language/AppImage a653a172 | a417c1f3 |
| Release notes b2c68c18 | d63542eb and 4d93df47, local prerelease support preserved |
| WebSocket recovery 0c186044 | e5eb725f |
| #490 fcc15021 | e97e81ee |
| #461 40e545c6 | 07412e92 |
| #488 5e26cb39 | dd48885e |
| #489 4e63e68d | bff70081; this change corrects empty-state colspan for the retained IP column |
| #493 161ba792 | c0f45bcc |
| #494 86aa0108 | 888db66c |
| #498 31bf5c31 | 76eccd07 |
| #495 70e551fb | 850f682d |
| #496 3b16432d | 27326cc7 |
| #487 fabd7ba3 | 6060d427 |
| #464 7c22c7b8 | a36d2d0c |
| #491 e6775a51 | 0c9e0d93 and 0b3ab34c |
| #492 9d03ca83 | a091f146 and 0b3ab34c; this change restores fifth-model ordering assertion |
| #502 81bc54f0 | Ported with native error/long terminal frame regressions |
| #504 47933856 | Ported with historical web search regressions |
| v0.6.4 596d9761 | Version/locks and four upstream changelogs synchronized, local identity untouched |

## Regression Cause and Prevention

The IP gate previously marked a request running before it acquired an account
slot. Move that transition to successful account acquisition, mark account waits
queued, and keep separate accumulated queue/running durations across retries.
Account selection is snapshotted in memory and cleared when switching to an
aggregate provider. Regression tests cover saturated accounts, reassignment,
aggregate fallback, repeated queue stages, and accumulated durations.

Log IP text previously used truncation within a fixed-layout table. Keep IP text
nonshrinking and nowrap, use automatic column sizing, and retain horizontal table
scrolling. Desktop-transport Playwright coverage checks text rectangles against
their cells at 1440px and 640px, including full IPv6 and IPv4.

Review found additional edge cases in the imported fixes: terminal words inside
normal output could suppress failure delivery; a truncated upstream frame could
absorb the generated failure; and native search replay could omit `tools`
entirely. Regression tests reproduce each before correction. Native terminal
delivery now reuses the existing eventsource-stream parser on downstream-bound
bytes, independent of the asynchronous usage observer. Failure emission separates
the previous frame, and missing native tool lists receive the same safe search
declaration with new tool calls disabled.

The delivery parser adds one frame buffer per native Responses stream; no
production throughput benchmark is claimed. The displayed running count retains
the existing bridge/accounting lifecycle and is not a direct measurement of
provider socket activity. Team labels do not change routing or concurrency scope.

## Validation Evidence

- Red checks: upstream historical search (3 failures), native terminal handling
  (2 failures), split large terminal frame (1 failure), identity/timing/account
  queue (3 failures), ordinary text containing terminal words (1 failure),
  truncated frame delivery (1 failure), omitted native tools (1 failure), and
  exhausted Tokio cooperative budget (1 failure). Each failure was inspected
  before its corresponding fix.
- Frontend runtime suite: 276 passed.
- Desktop static export and desktop-transport Playwright: 2 passed at 1440px and
  640px, including screenshots and text rectangle checks. These use IPC fixtures,
  not a real Tauri WebView or external account/provider traffic.
- ESLint for affected frontend implementation and desktop UI regression: clean.
- `cargo fmt --all -- --check`: passed. One existing settings import list was
  reformatted to keep the full workspace formatting check clean.
- `git -c core.whitespace=cr-at-eol diff --check`: passed, respecting existing
  Windows line endings.
- Initial default-thread workspace run: core 530 passed; service 1869 passed,
  8 ignored, 2 failed. The two aggregate tests timed out waiting for local mock
  HTTP requests (2-second limit); each passed on isolated rerun.
- Final `cargo test --workspace -- --test-threads=4`: exit 0; 2706 passed,
  0 failed, 19 ignored across 26 suite results. Both initially timed-out tests
  passed in this run. This does not establish default-thread stability.
- Protected README/sponsor assets, release workflows, local Tauri identity
  override, updater implementation, and migrations have no changes in this work.
- No commit, push, GitHub packaging, or real provider load test performed.
