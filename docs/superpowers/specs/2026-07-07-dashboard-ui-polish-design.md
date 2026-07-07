# Dashboard UI Polish Design

## Goal

Make the desktop dashboard feel like a refined light admin console while preserving the current navigation, service controls, data loading, and gateway behavior.

## Scope

- Keep the current blue console direction selected by the user.
- Reduce visual noise from repeated borders, scan lines, and dense blue accents.
- Improve hierarchy across the sidebar, header, dashboard metric cards, pool card, and usage analytics panel.
- Do not change API calls, routing, persistence, service startup behavior, or LAN IP usage logic.

## Files

- `apps/src/app/globals.css`: tune global console surfaces, background, glass cards, header/sidebar, and new dashboard polish utility classes.
- `apps/src/components/layout/sidebar.tsx`: make brand and navigation states calmer and more premium.
- `apps/src/components/layout/header.tsx`: reduce top-bar visual weight while preserving all controls.
- `apps/src/app/page.tsx`: apply refined metric, pool, and analytics panel classes.
- `apps/tests/dashboard-ui-polish.test.mjs`: source-level regression coverage for the intended visual structure.

## Validation

- Run the focused UI source test.
- Run the frontend build if dependencies are usable locally and no running app process must be started.

