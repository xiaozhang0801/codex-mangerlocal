import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

test("API Keys page reads token usage as one row per client IP", async () => {
  const pageSource = await readSource("src/app/apikeys/page.tsx");
  const serviceClientSource = await readSource("src/lib/api/service-client.ts");

  assert.match(serviceClientSource, /listClientIpUsage/);
  assert.match(pageSource, /clientIpUsageRows/);
  assert.match(pageSource, /todayTokensByClientIp/);
  assert.match(pageSource, /todayCostByClientIp/);
  assert.match(pageSource, /item\.clientIp/);
  assert.match(pageSource, /item\.todayEstimatedCostUsd/);
  assert.match(pageSource, /item\.estimatedCostUsd/);
  assert.match(pageSource, /今日 Token \/ 金额/);
  assert.match(pageSource, /累计 Token \/ 金额/);
  assert.match(pageSource, /内网 IP 用量/);
  assert.doesNotMatch(pageSource, /keyId:\s*item\.keyId/);
  assert.doesNotMatch(pageSource, /\[`${?item\.keyId}?:\${?item\.clientIp/);
});
