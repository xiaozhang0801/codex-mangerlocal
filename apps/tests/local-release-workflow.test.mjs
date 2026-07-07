import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";

const appsRoot = path.resolve(import.meta.dirname, "..");
const repoRoot = path.resolve(appsRoot, "..");
const localConfigPath = path.join(
  appsRoot,
  "src-tauri",
  "tauri.local.conf.json"
);
const workflowPath = path.join(repoRoot, ".github", "workflows", "release-local.yml");

assert.equal(
  existsSync(localConfigPath),
  true,
  "Local Tauri config should exist for CodexManagerLocal builds."
);

const localConfig = JSON.parse(readFileSync(localConfigPath, "utf8"));
assert.equal(localConfig.productName, "CodexManagerLocal");
assert.equal(localConfig.identifier, "com.codexmanager.local");
assert.equal(localConfig.app?.windows?.[0]?.title, "CodexManager Local");

assert.equal(
  existsSync(workflowPath),
  true,
  "release-local workflow should exist for CodexManagerLocal builds."
);

const workflowSource = readFileSync(workflowPath, "utf8");

assert.match(workflowSource, /^name:\s*release-local/m);
assert.match(workflowSource, /CodexManagerLocal/);
assert.match(workflowSource, /codexmanagerlocal-windows-x64/);
assert.match(workflowSource, /codexmanagerlocal-macos-x64/);
assert.match(
  workflowSource,
  /build\s+--bundles\s+nsis\s+--config\s+tauri\.local\.conf\.json\s+--ci/,
  "Windows build must pass the local Tauri config."
);
assert.match(
  workflowSource,
  /build\s+--bundles\s+app\s+--target\s+x86_64-apple-darwin\s+--config\s+tauri\.local\.conf\.json\s+--ci/,
  "macOS x64 build must pass the local Tauri config."
);
assert.doesNotMatch(
  workflowSource,
  /CodexManager\.app/,
  "Local workflow should not package the upstream app bundle name."
);
assert.doesNotMatch(
  workflowSource,
  /CodexManager_\$\{?version\}?/,
  "Local workflow should not stage upstream release asset names."
);
