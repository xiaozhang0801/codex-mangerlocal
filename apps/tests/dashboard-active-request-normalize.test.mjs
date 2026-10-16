import assert from "node:assert/strict";
import fs from "node:fs/promises";
import vm from "node:vm";
import test from "node:test";
import ts from "../node_modules/typescript/lib/typescript.js";

async function readActiveItem(item) {
  const source = await fs.readFile(new URL("../src/lib/api/dashboard-client.ts", import.meta.url), "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  });
  const exports = {};
  vm.runInNewContext(compiled.outputText, {
    exports,
    require(name) {
      if (name === "./normalize") return {};
      assert.equal(name, "./transport");
      return {
        withAddr: (params) => ({ addr: "127.0.0.1:48760", ...params }),
        invoke: async (command, params) => {
          assert.equal(command, "service_dashboard_active_requests");
          assert.equal(params.addr, "127.0.0.1:48760");
          return { items: [{ id: "trace-1", ...item }] };
        },
      };
    },
  });
  const result = await exports.dashboardClient.getActiveRequests();
  return result.items[0];
}

for (const snakeCase of [false, true]) {
  test(`active request preserves account/team/IP (${snakeCase ? "snake" : "camel"} case)`, async () => {
    const ip = "2001:0db8:85a3:0000:0000:8a2e:0370:7334";
    const item = await readActiveItem(snakeCase ? {
      account_id: "account-a", account_label: "owner@example.com", team_name: "Software",
      workspace_id: "workspace-a", client_ip: ip,
    } : {
      accountId: "account-a", accountLabel: "owner@example.com", teamName: "Software",
      workspaceId: "workspace-a", clientIp: ip,
    });
    assert.equal(item.accountId, "account-a");
    assert.equal(item.accountLabel, "owner@example.com");
    assert.equal(item.teamName, "Software");
    assert.equal(item.workspaceId, "workspace-a");
    assert.equal(item.clientIp, ip);
  });
}

test("older service replies leave account metadata unknown", async () => {
  const item = await readActiveItem({ sourceKind: "aggregate_api", sourceId: "provider-a" });
  assert.equal(item.accountId, null);
  assert.equal(item.accountLabel, null);
  assert.equal(item.teamName, null);
  assert.equal(item.workspaceId, null);
});
