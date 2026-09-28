import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

async function readSource(relativePath) {
  return fs.readFile(path.join(appsRoot, relativePath), "utf8");
}

test("request log normalization maps client_ip to clientIp", async () => {
  const normalizeSource = await readSource("src/lib/api/normalize.ts");
  const requestLogTypesSource = await readSource("src/types/request-log.ts");

  assert.match(requestLogTypesSource, /clientIp:\s*string/);
  assert.match(normalizeSource, /clientIp:\s*asString\(source\.clientIp\s*\?\?\s*source\.client_ip\)/);
});

test("client IP usage normalization returns IP-only rows without key id", async () => {
  const normalizeSource = await readSource("src/lib/api/normalize.ts");
  const requestLogTypesSource = await readSource("src/types/request-log.ts");

  assert.match(requestLogTypesSource, /interface ClientIpUsageSummary/);
  assert.match(requestLogTypesSource, /interface ClientIpUsageListResult/);
  assert.match(normalizeSource, /normalizeClientIpUsageListResult/);
  assert.match(normalizeSource, /clientIp:\s*asString\(source\.clientIp\s*\?\?\s*source\.client_ip\)/);
  assert.doesNotMatch(normalizeSource, /keyId:\s*asString\(source\.keyId\s*\?\?\s*source\.key_id\)/);
});

test("client IP usage keeps the aggregate counters required by the App", async () => {
  const requestLogTypesSource = await readSource("src/types/request-log.ts");
  const rpcTypesSource = await readSource("../crates/core/src/rpc/types.rs");

  for (const field of [
    "totalTokens",
    "todayTokens",
    "requestCount",
    "successCount",
    "errorCount",
    "estimatedCostUsd",
  ]) {
    assert.match(requestLogTypesSource, new RegExp(field));
  }
  assert.match(rpcTypesSource, /ClientIpUsageListParams/);
  assert.match(rpcTypesSource, /ClientIpUsageSummaryResult/);
});
