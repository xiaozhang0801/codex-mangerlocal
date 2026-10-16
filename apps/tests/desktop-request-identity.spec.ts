import { expect, test, type Locator, type Page } from "@playwright/test";
import { personnelWorkbook } from "./support/personnel-workbook.cjs";

const IPV6 = "2001:0db8:1234:5678:9abc:def0:1234:5678";
const IPV4 = "192.168.123.234";

async function mockDesktop(page: Page) {
  await page.addInitScript(({ ipv6, ipv4 }) => {
    const runtime = window as unknown as Record<string, unknown>;
    const settings = () => ({ locale: "zh-CN", theme: "tech", appearancePreset: "classic", serviceAddr: "localhost:48760", codexCliGuideDismissed: true, updateAutoCheck: false, lowTransparency: true,
      clientIpNames: JSON.parse(sessionStorage.getItem("fixture-ip-names") ?? JSON.stringify({ [ipv6]: "Alice" })) });
    const active = [
      { id: "selected", clientIp: ipv6, status: "running", accountId: "account-a", accountLabel: "owner@example.com", teamName: "软件组", workspaceId: "workspace-a", sourceKind: "openai_account" },
      { id: "waiting", clientIp: ipv4, status: "queued" },
      { id: "personal", clientIp: ipv4, status: "running", accountId: "personal-b", sourceKind: "openai_account" },
      { id: "aggregate", clientIp: ipv4, status: "running", sourceKind: "aggregate_api", sourceId: "provider-a" },
      { id: "workspace", clientIp: ipv4, status: "queued", accountId: "account-c", workspaceId: "workspace-c", sourceKind: "openai_account" },
    ].map(item => ({ traceId: item.id, keyId: "shared-key", path: "/v1/responses", method: "POST", model: "gpt-test", waitMs: 2000, runningMs: 3000, ...item }));
    const logs = [ipv6, ipv4].map((client_ip, index) => ({
      id: index + 1, trace_id: `log-${index}`, client_ip, key_id: "shared-key",
      request_path: "/v1/responses", method: "POST", model: "gpt-test",
      status_code: 200, duration_ms: 2345, first_response_ms: 340,
      total_tokens: 154, created_at: 1792000000,
    }));
    let callbackId = 0;
    runtime.__TAURI_INTERNALS__ = {
      metadata: { currentWindow: { label: "main" }, currentWebview: { label: "main" } },
      transformCallback: () => ++callbackId,
      unregisterCallback: () => {},
      invoke: async (command: string, args?: { patch?: { clientIpNames?: Record<string, string> } }) => {
        if (command.startsWith("plugin:")) return null;
        switch (command) {
          case "app_settings_get":
            return settings();
          case "app_settings_set":
            if (runtime.__failBindingSave) throw new Error("fixture save failed");
            if (args?.patch?.clientIpNames) sessionStorage.setItem("fixture-ip-names", JSON.stringify(args.patch.clientIpNames));
            return settings();
          case "app_desktop_diagnostics":
          case "app_storage_space_summary":
            return {};
          case "service_initialize":
            return { version: "0.6.4", userAgent: "codex_cli_rs/test", codexHome: "C:/Test/.codex", platformFamily: "windows", platformOs: "windows" };
          case "service_account_manager_session_current":
            return { role: "system_admin", permissions: ["system:admin"] };
          case "service_dashboard_active_requests":
            return { totalCount: 5, runningCount: 3, queuedCount: 2, items: active, ipGroups: [
              { clientIp: ipv6, totalCount: 1, runningCount: 1, queuedCount: 0, maxWaitMs: 2000, maxRunningMs: 3000 },
              { clientIp: ipv4, totalCount: 4, runningCount: 2, queuedCount: 2, maxWaitMs: 2000, maxRunningMs: 3000 },
            ] };
          case "service_requestlog_list":
          case "service_requestlog_list_with_summary":
            return { items: logs, total: 2, page: 1, pageSize: 10, summary: { totalCount: 2, filteredCount: 2, successCount: 2, errorCount: 0 } };
          case "service_requestlog_client_ip_usage":
            return { items: [ipv6, ipv4].map((clientIp, index) => ({
              clientIp, requestCount: 10, successCount: 9, errorCount: 1,
              totalTokens: (index + 1) * 100, estimatedCostUsd: (index + 1) * 0.25,
              lastSeenAt: 1792000000,
            })) };
          case "service_startup_snapshot":
          case "service_dashboard_admin_usage_summary":
          case "service_requestlog_summary":
          case "service_requestlog_today_summary":
          case "service_requestlog_payload_queue_stats":
          case "service_gateway_concurrency_recommend_get":
          case "service_gateway_manual_account_get":
          case "app_update_status":
          case "app_show_main_window":
            return {};
          case "service_account_list":
          case "service_apikey_list":
          case "service_aggregate_api_list":
          case "service_apikey_usage_stats":
            return { items: [], total: 0 };
          default:
            throw new Error(`Unhandled desktop command: ${command}`);
        }
      },
    };
  }, { ipv6: IPV6, ipv4: IPV4 });
}

async function expectFullSingleLineIp(cell: Locator, ip: string) {
  await expect(cell).toContainText(ip);
  const geometry = await cell.evaluate((element, value) => {
    const walker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
    let node: Node | null;
    while ((node = walker.nextNode())) {
      if (node.textContent?.trim() !== value) continue;
      const range = document.createRange();
      range.selectNodeContents(node);
      const rect = range.getBoundingClientRect();
      const bounds = element.getBoundingClientRect();
      const style = getComputedStyle(node.parentElement!);
      return { lines: range.getClientRects().length, fits: rect.left >= bounds.left && rect.right <= bounds.right + 1, whiteSpace: style.whiteSpace, overflow: style.textOverflow };
    }
    return null;
  }, ip);
  expect(geometry).toMatchObject({ lines: 1, fits: true, whiteSpace: "nowrap" });
  expect(geometry?.overflow).not.toBe("ellipsis");
}

for (const width of [1440, 640]) {
  test(`LAN IP usage personnel labels stay complete at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 1000 });
    await mockDesktop(page);
    await page.goto("/apikeys/");
    const table = page.getByRole("table").filter({ has: page.getByRole("columnheader", { name: "客户端 IP", exact: true }) });
    for (const [ip, name] of [[IPV6, "Alice"], [IPV4, "未绑定"]]) {
      await expectFullSingleLineIp(table.getByRole("cell").filter({ hasText: ip }), `${ip}（${name}）`);
    }
    const rows = table.locator("tbody tr");
    await expect(rows.first()).toContainText(IPV4);
    for (const index of [1, 2, 3]) {
      await expect(rows.first().locator("td").nth(index)).toContainText("200");
      await expect(rows.first().locator("td").nth(index)).toContainText("$0.50");
    }
    const scroller = table.locator("..");
    expect(await scroller.evaluate(el => getComputedStyle(el).overflowX)).toBe("auto");
    if (width === 640) expect(await scroller.evaluate(el => el.scrollWidth > el.clientWidth)).toBe(true);
    await table.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `test-results/desktop-ip-usage-${width}.png`, fullPage: true, animations: "disabled" });
  });

  test(`desktop request identity and full single-line IP at ${width}px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 1000 });
    await mockDesktop(page);
    await page.goto("/");
    const table = page.getByRole("table").filter({ has: page.getByRole("columnheader", { name: "等待 / 运行" }) });
    await expect(table.getByRole("columnheader", { name: "账号", exact: true })).toBeVisible();
    await expect(table.getByRole("columnheader", { name: "团队", exact: true })).toBeVisible();
    await expect(table.getByRole("columnheader", { name: "来源", exact: true })).toHaveCount(0);
    const selected = table.getByRole("row").filter({ hasText: "owner@example.com" });
    await expect(selected).toContainText("软件组");
    await expect(selected).toContainText("2s / 3s");
    await expectFullSingleLineIp(selected.locator("td").nth(1), `${IPV6}（Alice）`);
    await expect(table.getByRole("cell").filter({ hasText: `${IPV4}（未绑定）` })).toHaveCount(4);
    await expect(table.getByText("待分配", { exact: true })).toBeVisible();
    await expect(table.getByText("未设置团队", { exact: true })).toBeVisible();
    await expect(table.getByText("workspace-c", { exact: true })).toBeVisible();
    await expect(table.getByText("provider-a", { exact: true })).toHaveCount(0);
    const aggregate = table.getByRole("row").filter({ has: page.locator("td:nth-child(4)").filter({ hasText: /^--$/ }) });
    await expect(aggregate).toHaveCount(1);
    await expect(aggregate.locator("td").nth(4)).toHaveText("--");
    await page.screenshot({ path: `test-results/desktop-active-requests-${width}.png`, fullPage: true, animations: "disabled" });

    await page.goto("/logs/");
    const logs = page.getByRole("table").filter({ has: page.getByRole("columnheader", { name: "客户端 IP" }) });
    for (const ip of [IPV6, IPV4]) {
      await expectFullSingleLineIp(logs.getByRole("cell").filter({ hasText: ip }), `${ip}（${ip === IPV6 ? "Alice" : "未绑定"}）`);
    }
    const scroller = logs.locator("..");
    expect(await scroller.evaluate(el => el.scrollWidth > el.clientWidth && getComputedStyle(el).overflowX === "auto")).toBe(true);
    await logs.getByText(`${IPV6}（Alice）`, { exact: true }).scrollIntoViewIfNeeded();
    await page.screenshot({ path: `test-results/desktop-request-logs-${width}.png`, fullPage: true, animations: "disabled" });
  });
}

test("personnel XLSX preview, failed save, replace, reload and clear", async ({ page }) => {
  await mockDesktop(page);
  await page.goto("/settings/");
  const section = page.getByRole("region", { name: "IP 人员绑定" });
  await expect(section).toBeVisible();
  await expect(section).toContainText("Alice");
  await section.locator('input[type="file"]').setInputFiles({
    name: "people.xlsx", mimeType: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    buffer: personnelWorkbook([["Personnel"], ["姓名", "IPv4 地址(IP)"], ["Bob", IPV4], ["Missing", ""]]),
  });
  await expect(section).toContainText("Bob");
  await expect(section).toContainText("跳过 1");
  await page.evaluate(() => { (window as unknown as Record<string, unknown>).__failBindingSave = true; });
  await section.getByRole("button", { name: "确认替换" }).click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("button", { name: "确定", exact: true }).click();
  await expect(page.getByText("保存失败，原有绑定未更改", { exact: true })).toBeVisible();
  expect(await page.evaluate(() => sessionStorage.getItem("fixture-ip-names"))).toBeNull();
  await page.evaluate(() => { (window as unknown as Record<string, unknown>).__failBindingSave = false; });
  await dialog.getByRole("button", { name: "确定", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(section.getByText("Bob", { exact: true })).toBeVisible();
  await page.goto("/");
  await expect(page.getByRole("cell", { name: `${IPV4}（Bob）`, exact: true })).toHaveCount(4);
  await page.goto("/logs/");
  await expect(page.getByText(`${IPV4}（Bob）`, { exact: true })).toBeVisible();
  await expect(page.getByText(`${IPV6}（未绑定）`, { exact: true })).toBeVisible();
  await page.goto("/apikeys/");
  await expect(page.getByText(`${IPV4}（Bob）`, { exact: true })).toBeVisible();
  await expect(page.getByText(`${IPV6}（未绑定）`, { exact: true })).toBeVisible();
  await page.goto("/settings/");
  await section.getByRole("button", { name: "清空绑定" }).click();
  await dialog.getByRole("button", { name: "确定", exact: true }).click();
  await expect(dialog).toBeHidden();
  await page.reload();
  await expect(section).toContainText("已绑定 0");
  await page.goto("/apikeys/");
  await expect(page.getByText(`${IPV4}（未绑定）`, { exact: true })).toBeVisible();
});

test("invalid or empty imports never replace existing bindings", async ({ page }) => {
  await mockDesktop(page);
  await page.goto("/settings/");
  const section = page.getByRole("region", { name: "IP 人员绑定" });
  for (const rows of [
    [["姓名", "IP"], ["Bob", "bad-ip"]],
    [["姓名", "IP"], ["Bob", ""]],
    [["姓名", "IP"], ["Bob", "192.0.2.1"], ["Eve", "192.0.2.1"]],
  ]) {
    await section.locator('input[type="file"]').setInputFiles({ name: "bad.xlsx", mimeType: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", buffer: personnelWorkbook(rows) });
    await expect(section.getByRole("alert")).toBeVisible();
    await expect(section).toContainText("Alice");
    await expect(section.getByRole("button", { name: "确认替换" })).toHaveCount(0);
  }
  expect(await page.evaluate(() => sessionStorage.getItem("fixture-ip-names"))).toBeNull();
});
