import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const source = readFileSync("src/app/apikeys/page.tsx", "utf8");

assert.match(source, /listClientIpUsage/);
assert.match(source, /内网 IP 用量/);
assert.match(source, /clientIpUsage/);
assert.match(source, /todayClientIpUsage/);
assert.match(source, /今日 Token/);
assert.match(source, /startTs:\s*clientIpTodayRange\.startTs/);
assert.match(source, /endTs:\s*clientIpTodayRange\.endTs/);
assert.match(source, /lastSeenAt/);
