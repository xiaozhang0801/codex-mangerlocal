import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");
const repoRoot = path.resolve(appsRoot, "..");
const defaultConfigPath = path.join(appsRoot, "src-tauri", "tauri.conf.json");
const desktopCargoPath = path.join(appsRoot, "src-tauri", "Cargo.toml");
const localConfigPath = path.join(appsRoot, "src-tauri", "tauri.local.conf.json");
const updaterRuntimePath = path.join(
  appsRoot,
  "src-tauri",
  "src",
  "commands",
  "updater",
  "runtime.rs",
);
const envOverrideCatalogPath = path.join(
  repoRoot,
  "crates",
  "service",
  "src",
  "app_settings",
  "env_overrides",
  "catalog",
  "items.rs",
);
const workflowPath = path.join(repoRoot, ".github", "workflows", "release-local.yml");
const macosHelperPath = path.join(
  repoRoot,
  "assets",
  "macos-local",
  "Open CodexManagerLocal.command",
);
const macosReadmePath = path.join(
  repoRoot,
  "assets",
  "macos-local",
  "README-macOS-first-launch-local.txt",
);

test("CodexManagerLocal has isolated Tauri identity and local release workflow", () => {
  assert.equal(existsSync(defaultConfigPath), true);
  const defaultConfig = JSON.parse(readFileSync(defaultConfigPath, "utf8"));
  assert.equal(defaultConfig.productName, "CodexManagerLocal");
  assert.equal(defaultConfig.identifier, "com.codexmanager.local");
  assert.equal(defaultConfig.app?.windows?.[0]?.title, "CodexManager Local");

  assert.equal(existsSync(desktopCargoPath), true);
  const desktopCargoSource = readFileSync(desktopCargoPath, "utf8");
  assert.match(desktopCargoSource, /^name\s*=\s*"CodexManagerLocal"/m);

  assert.equal(existsSync(localConfigPath), true);
  const localConfig = JSON.parse(readFileSync(localConfigPath, "utf8"));
  assert.equal(localConfig.productName, "CodexManagerLocal");
  assert.equal(localConfig.identifier, "com.codexmanager.local");
  assert.equal(localConfig.app?.windows?.[0]?.title, "CodexManager Local");

  assert.equal(existsSync(workflowPath), true);
  const workflowSource = readFileSync(workflowPath, "utf8");
  assert.match(workflowSource, /^name:\s*release-local/m);
  assert.match(workflowSource, /CodexManagerLocal/);
  assert.match(workflowSource, /codexmanagerlocal-windows-x64/);
  assert.match(workflowSource, /codexmanagerlocal-macos-x64/);
  assert.match(workflowSource, /--config\s+src-tauri\/tauri\.local\.conf\.json/);
  assert.match(workflowSource, /CodexManagerLocal_\$\{version\}_x64-setup\.exe/);
  assert.match(workflowSource, /CodexManagerLocal_\$\{version\}_x64\.dmg/);
  assert.doesNotMatch(workflowSource, /CodexManager\.app/);
  assert.doesNotMatch(workflowSource, /CodexManager_\$\{?version\}?/);

  assert.equal(existsSync(macosHelperPath), true);
  const macosHelperSource = readFileSync(macosHelperPath, "utf8");
  assert.match(macosHelperSource, /CodexManagerLocal\.app/);
  assert.match(macosHelperSource, /xattr -dr com\.apple\.quarantine/);

  assert.equal(existsSync(macosReadmePath), true);
  assert.match(
    readFileSync(macosReadmePath, "utf8"),
    /CodexManagerLocal macOS first launch/,
  );
});

test("CodexManagerLocal updater defaults to the local GitHub releases", () => {
  const localRepo = "xiaozhang0801/codex-mangerlocal";

  assert.equal(existsSync(updaterRuntimePath), true);
  const updaterRuntimeSource = readFileSync(updaterRuntimePath, "utf8");
  assert.match(
    updaterRuntimeSource,
    new RegExp(`DEFAULT_UPDATE_REPO: &str = "${localRepo}"`),
  );
  assert.doesNotMatch(updaterRuntimeSource, /DEFAULT_UPDATE_REPO: &str = "qxcnm\/Codex-Manager"/);

  assert.equal(existsSync(envOverrideCatalogPath), true);
  const envOverrideCatalogSource = readFileSync(envOverrideCatalogPath, "utf8");
  assert.match(
    envOverrideCatalogSource,
    new RegExp(`"CODEXMANAGER_UPDATE_REPO",[\\s\\S]*"${localRepo}"`),
  );
});
