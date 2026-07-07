import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const dashboardSource = readFileSync("src/app/page.tsx", "utf8");
const sidebarSource = readFileSync("src/components/layout/sidebar.tsx", "utf8");
const headerSource = readFileSync("src/components/layout/header.tsx", "utf8");
const globalsSource = readFileSync("src/app/globals.css", "utf8");

assert.match(dashboardSource, /dashboard-metric-card/);
assert.match(dashboardSource, /dashboard-pool-card/);
assert.match(dashboardSource, /dashboard-analytics-card/);

assert.match(sidebarSource, /console-nav-item/);
assert.match(sidebarSource, /console-brand-surface/);
assert.match(headerSource, /console-control-surface/);

assert.match(globalsSource, /\.dashboard-metric-card/);
assert.match(globalsSource, /\.dashboard-pool-card/);
assert.match(globalsSource, /\.dashboard-analytics-card/);
assert.match(globalsSource, /\.console-nav-item/);
assert.match(globalsSource, /\.console-control-surface/);
