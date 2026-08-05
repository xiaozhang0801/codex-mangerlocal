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
