import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const normalizeSource = readFileSync("src/lib/api/normalize.ts", "utf8");
const typesSource = readFileSync("src/types/request-log.ts", "utf8");
const serviceClientSource = readFileSync("src/lib/api/service-client.ts", "utf8");

assert.match(typesSource, /clientIp:\s*string/);
assert.match(typesSource, /interface ClientIpUsageSummary/);
assert.match(
  normalizeSource,
  /clientIp:\s*asString\(source\.clientIp\s*\?\?\s*source\.client_ip\)/,
);
assert.match(normalizeSource, /normalizeClientIpUsageListResult/);
assert.match(serviceClientSource, /listClientIpUsage/);
assert.match(serviceClientSource, /service_requestlog_client_ip_usage/);
