import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

test("桌面端注册并调用实时活跃请求 RPC", async () => {
  const [commandSource, registrySource, clientSource] = await Promise.all([
    readSource("src-tauri/src/commands/dashboard.rs"),
    readSource("src-tauri/src/commands/registry.rs"),
    readSource("src/lib/api/dashboard-client.ts"),
  ]);

  assert.match(commandSource, /service_dashboard_active_requests/);
  assert.match(commandSource, /dashboard\/activeRequests/);
  assert.match(registrySource, /::service_dashboard_active_requests\b/);
  assert.match(clientSource, /service_dashboard_active_requests/);
  assert.match(clientSource, /getActiveRequests/);
  assert.match(clientSource, /readActiveRequests/);
});

test("实时活跃请求 hook 仅桌面端启用并轮询", async () => {
  const hookSource = await readSource("src/hooks/useDashboardActiveRequests.ts");

  assert.match(hookSource, /useRuntimeCapabilities/);
  assert.match(hookSource, /isDesktopRuntime/);
  assert.match(hookSource, /enabled && isServiceReady && isPageActive && isDesktopRuntime/);
  assert.match(hookSource, /refetchInterval: isQueryEnabled \? 1500 : false/);
});

test("管理员首页展示 IP 活跃请求，成员首页不挂载", async () => {
  const pageSource = await readSource("src/app/page.tsx");
  const adminDashboardSource = pageSource.slice(
    pageSource.indexOf("function AdminDashboard()"),
    pageSource.indexOf("function MemberDashboard()"),
  );
  const memberDashboardSource = pageSource.slice(
    pageSource.indexOf("function MemberDashboard()"),
    pageSource.indexOf("interface MemberAlertListProps"),
  );

  assert.match(pageSource, /useDashboardActiveRequests/);
  assert.match(pageSource, /DashboardActiveRequestIpGroup/);
  assert.match(adminDashboardSource, /useDashboardActiveRequests/);
  assert.match(adminDashboardSource, /<AdminActiveRequestsCard/);
  assert.doesNotMatch(memberDashboardSource, /useDashboardActiveRequests/);
  assert.doesNotMatch(memberDashboardSource, /<AdminActiveRequestsCard/);
});

test("实时请求卡片按 IP 分组显示运行与排队状态", async () => {
  const pageSource = await readSource("src/app/page.tsx");

  assert.match(pageSource, /function AdminActiveRequestsCard/);
  assert.match(pageSource, /ipGroups/);
  assert.match(pageSource, /运行中/);
  assert.match(pageSource, /排队中/);
  assert.match(pageSource, /clientIp/);
  assert.match(pageSource, /waitMs/);
  assert.match(pageSource, /runningMs/);
});
