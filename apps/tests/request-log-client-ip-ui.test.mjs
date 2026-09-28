import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

test("request logs table shows and searches client IP", async () => {
  const sectionsSource = await readSource("src/app/logs/page-sections.tsx");
  const cellsSource = await readSource("src/app/logs/page-cells.tsx");

  assert.match(sectionsSource, /客户端 IP/);
  assert.match(sectionsSource, /搜索路径、账号、密钥 ID 或 IP/);
  assert.match(sectionsSource, /ClientIpCell/);
  assert.match(cellsSource, /export function ClientIpCell/);
  assert.match(cellsSource, /log\.clientIp/);
});

test("desktop request-log API exposes the client IP usage RPC", async () => {
  const [commandSource, registrySource, clientSource] = await Promise.all([
    readSource("src-tauri/src/commands/requestlog.rs"),
    readSource("src-tauri/src/commands/registry.rs"),
    readSource("src/lib/api/service-client.ts"),
  ]);

  assert.match(commandSource, /requestlog\/client_ip_usage/);
  assert.match(commandSource, /service_requestlog_client_ip_usage/);
  assert.match(registrySource, /service_requestlog_client_ip_usage/);
  assert.match(clientSource, /listClientIpUsage/);
});
