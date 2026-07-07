import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const clientSource = readFileSync("src/lib/api/dashboard-client.ts", "utf8");
const hookSource = readFileSync("src/hooks/useDashboardActiveRequests.ts", "utf8");
const pageSource = readFileSync("src/app/page.tsx", "utf8");

const adminDashboardSource = pageSource.slice(
  pageSource.indexOf("function AdminDashboard()"),
  pageSource.indexOf("function MemberDashboard()"),
);
const memberDashboardSource = pageSource.slice(
  pageSource.indexOf("function MemberDashboard()"),
  pageSource.indexOf("function DashboardPage()"),
);

assert.match(
  clientSource,
  /service_dashboard_active_requests/,
  "Dashboard client should call the desktop active requests command.",
);

assert.match(
  hookSource,
  /isDesktopRuntime/,
  "Active requests hook should be gated to the desktop runtime.",
);

assert.match(
  hookSource,
  /refetchInterval:\s*isQueryEnabled\s*\?\s*1500\s*:\s*false/,
  "Active requests hook should poll only while enabled.",
);

assert.match(
  adminDashboardSource,
  /useDashboardActiveRequests/,
  "Admin dashboard should mount the active requests hook.",
);

assert.doesNotMatch(
  memberDashboardSource,
  /useDashboardActiveRequests/,
  "Member dashboard should not mount the active requests hook.",
);

assert.match(
  adminDashboardSource,
  /运行中/,
  "Admin dashboard should show running request text.",
);

assert.match(
  adminDashboardSource,
  /排队中/,
  "Admin dashboard should show queued request text.",
);

assert.match(
  adminDashboardSource,
  /暂无进行中的请求/,
  "Admin dashboard should show an empty state for active requests.",
);
