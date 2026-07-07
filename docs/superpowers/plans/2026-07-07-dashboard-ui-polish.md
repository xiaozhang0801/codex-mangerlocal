# Dashboard UI Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Polish the desktop dashboard into a refined light admin console without changing behavior.

**Architecture:** Keep the existing Next.js/Tailwind/Tauri shell and add a small set of semantic dashboard polish classes. Components keep their current data flow and only receive class name changes.

**Tech Stack:** Next.js 16 App Router, React 19, Tailwind CSS v4, shadcn/Base UI, lucide-react.

---

### Task 1: Source-Level Visual Contract

**Files:**
- Create: `apps/tests/dashboard-ui-polish.test.mjs`

- [ ] Add assertions that the dashboard uses refined metric, pool, and analytics panel classes.
- [ ] Add assertions that the sidebar and header use calmer console class hooks.
- [ ] Run `node --test tests/dashboard-ui-polish.test.mjs` from `apps` and confirm it fails before implementation.

### Task 2: Global Console Surface Polish

**Files:**
- Modify: `apps/src/app/globals.css`

- [ ] Reduce background scanline/grid intensity.
- [ ] Improve `glass-card`, `glass-sidebar`, and `glass-header` depth without increasing blur cost.
- [ ] Add `.dashboard-metric-card`, `.dashboard-pool-card`, `.dashboard-analytics-card`, and `.console-control-surface`.

### Task 3: Shell Component Polish

**Files:**
- Modify: `apps/src/components/layout/sidebar.tsx`
- Modify: `apps/src/components/layout/header.tsx`

- [ ] Apply the new shell class hooks to brand, nav items, notice, and port controls.
- [ ] Preserve all buttons, inputs, links, labels, and event handlers.

### Task 4: Dashboard Panel Polish

**Files:**
- Modify: `apps/src/app/page.tsx`

- [ ] Apply refined classes to admin and member metric cards.
- [ ] Apply the new pool card style to the account pool remaining strip.
- [ ] Apply the analytics panel style to the admin usage chart card.

### Task 5: Validation

**Commands:**
- `node --test tests/dashboard-ui-polish.test.mjs`
- `pnpm -C apps run build`

- [ ] Report exact command results and any environment limitation.

