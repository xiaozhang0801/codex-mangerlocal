import { expect, test } from "@playwright/test";

test("settings hides legacy auth cleanup and preserves unrelated preferences", async ({ page }) => {
  let settings: Record<string, unknown> = {
    locale: "en",
    codexCliGuideDismissed: true,
    serviceAddr: "localhost:48760",
    theme: "tech",
    removeRequiresOpenaiAuth: true,
  };
  const patches: Record<string, unknown>[] = [];
  await page.route(/\/api\/runtime\/?(?:\?.*)?$/, (route) => route.fulfill({
    json: { mode: "web-gateway", rpcBaseUrl: "/api/rpc", canManageService: false },
  }));
  await page.route(/\/api\/rpc\/?(?:\?.*)?$/, async (route) => {
    const { id, method, params } = route.request().postDataJSON();
    let result: unknown = {};
    if (method === "appSettings/get") result = settings;
    if (method === "appSettings/set") {
      patches.push(params);
      settings = { ...settings, ...params };
      result = settings;
    }
    if (method === "initialize") result = { userAgent: "codex_cli_rs/test", codexHome: "C:/Test/.codex" };
    if (method === "accountManager/session/current") {
      result = { mode: "none", role: "system_admin", permissions: [], distributionEnabled: false };
    }
    await route.fulfill({ json: { jsonrpc: "2.0", id, result } });
  });
  await page.goto("/settings/");
  const legacyControl = page.getByRole("switch", { name: "Remove requires_openai_auth", exact: true });
  const performanceControl = page.getByRole("switch", { name: "Visual performance mode", exact: true });
  await expect(performanceControl).toBeVisible();
  await expect(legacyControl).toHaveCount(0);
  await performanceControl.click();
  await expect.poll(() => patches.some((patch) => patch.lowTransparency === true)).toBe(true);
  expect(patches.every((patch) => !("removeRequiresOpenaiAuth" in patch))).toBe(true);
  await page.reload();
  await expect(performanceControl).toBeChecked();
  await expect(legacyControl).toHaveCount(0);

});
