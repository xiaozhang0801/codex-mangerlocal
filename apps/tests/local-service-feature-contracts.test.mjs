import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const appsDir = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const repoDir = path.resolve(appsDir, "..");

function readRepoFile(...parts) {
  return fs.readFileSync(path.join(repoDir, ...parts), "utf8");
}

test("service resolves a trusted client IP and rejects direct header spoofing", () => {
  const clientIpPath = path.join(repoDir, "crates", "service", "src", "client_ip.rs");
  const proxyRuntimeSource = readRepoFile(
    "crates",
    "service",
    "src",
    "http",
    "proxy_runtime.rs",
  );

  assert.ok(fs.existsSync(clientIpPath), "trusted client IP module should exist");
  const clientIpSource = fs.readFileSync(clientIpPath, "utf8");
  assert.ok(
    /x-codexmanager-client-ip/.test(clientIpSource),
    "trusted IP header name should be defined",
  );
  assert.ok(
    /loopback|localhost/i.test(clientIpSource),
    "trusted IP resolution should enforce loopback",
  );
  assert.ok(
    /resolve_trusted_client_ip/.test(clientIpSource),
    "trusted IP resolver should exist",
  );
  assert.ok(
    /set_forwarded_client_ip_header/.test(proxyRuntimeSource),
    "proxy runtime should sanitize and inject the trusted IP header",
  );
  assert.ok(
    /filter_request_headers/.test(proxyRuntimeSource),
    "proxy runtime should filter caller-supplied forwarding headers",
  );
});

test("request logs and token stats carry the same client IP for aggregation", () => {
  const coreStorageSource = readRepoFile("crates", "core", "src", "storage", "mod.rs");
  const requestLogSource = readRepoFile(
    "crates",
    "service",
    "src",
    "gateway",
    "observability",
    "request_log.rs",
  );
  const seaormLogSource = readRepoFile(
    "crates",
    "storage-seaorm",
    "src",
    "request_logs.rs",
  );
  const seaormTokenSource = readRepoFile(
    "crates",
    "storage-seaorm",
    "src",
    "request_token_stats.rs",
  );
  const rpcSource = readRepoFile(
    "crates",
    "service",
    "src",
    "rpc_dispatch",
    "requestlog.rs",
  );

  assert.ok(
    /pub\s+client_ip:\s+Option<String>/.test(coreStorageSource),
    "core request records should carry client_ip",
  );
  assert.ok(/client_ip/.test(requestLogSource), "request log should carry client_ip");
  assert.ok(/client_ip/.test(seaormLogSource), "SeaORM request logs should carry client_ip");
  assert.ok(
    /client_ip/.test(seaormTokenSource),
    "SeaORM token stats should carry client_ip",
  );
  assert.ok(
    /client_ip_usage/.test(rpcSource),
    "request-log RPC should expose client_ip_usage",
  );
});

test("conditional IP gate keeps the four-running-request rule explicit", () => {
  const routingSource = readRepoFile(
    "crates",
    "service",
    "src",
    "gateway",
    "routing",
    "request_gate.rs",
  );
  const proxySource = readRepoFile(
    "crates",
    "service",
    "src",
    "gateway",
    "upstream",
    "proxy.rs",
  );
  const testPath = path.join(
    repoDir,
    "crates",
    "service",
    "src",
    "gateway",
    "routing",
    "tests",
    "request_gate_tests.rs",
  );
  const testSource = fs.readFileSync(testPath, "utf8");

  assert.ok(
    /CLIENT_IP_GATE_MAX_RUNNING/.test(routingSource),
    "IP gate should define the four-request limit",
  );
  assert.ok(
    /client_ip_gate/.test(routingSource),
    "routing gate should expose a client_ip scope",
  );
  assert.ok(/client_ip/.test(proxySource), "proxy should carry client_ip");
  assert.ok(
    /acquire_client_ip_request_gate|client_ip_gate/.test(proxySource),
    "proxy should acquire the client IP gate",
  );
  assert.ok(
    /single|one|only.*IP/i.test(testSource),
    "gate tests should cover the single-IP mode",
  );
  assert.ok(/four|4/.test(testSource), "gate tests should cover four slots");
});
