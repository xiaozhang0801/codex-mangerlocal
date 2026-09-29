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

test("Tauri desktop identity is isolated for CodexManagerLocal", () => {
  const config = JSON.parse(readRepoFile("apps", "src-tauri", "tauri.conf.json"));

  assert.equal(config.productName, "CodexManagerLocal");
  assert.equal(config.identifier, "com.codexmanager.local");
  assert.equal(config.app.windows[0].title, "CodexManager Local");
});

test("local updater defaults and release workflow target the local repository", () => {
  const updaterSource = readRepoFile(
    "apps",
    "src-tauri",
    "src",
    "commands",
    "updater",
    "runtime.rs",
  );
  const githubSource = readRepoFile(
    "apps",
    "src-tauri",
    "src",
    "commands",
    "updater",
    "github.rs",
  );
  const workflowPath = path.join(repoDir, ".github", "workflows", "release-local.yml");
  assert.ok(fs.existsSync(workflowPath), "local release workflow should exist");
  const workflowSource = fs.readFileSync(workflowPath, "utf8");

  assert.match(updaterSource, /xiaozhang0801\/codex-mangerlocal/);
  assert.match(githubSource, /xiaozhang0801\/codex-mangerlocal/);
  assert.match(workflowSource, /CodexManagerLocal/);
  assert.match(workflowSource, /tauri\.local\.conf\.json/);
  assert.match(workflowSource, /x64-setup\.exe/);
  assert.match(workflowSource, /x64\.dmg/);
  assert.doesNotMatch(updaterSource, /qxcnm\/Codex-Manager/);
  assert.doesNotMatch(githubSource, /qxcnm\/Codex-Manager/);
  assert.doesNotMatch(workflowSource, /qxcnm\/Codex-Manager/);
});

test("local release workflow serializes tags and validates the selected ref", () => {
  const workflowSource = readRepoFile(
    ".github",
    "workflows",
    "release-local.yml",
  );

  assert.match(workflowSource, /concurrency:/);
  assert.match(workflowSource, /cancel-in-progress:\s*false/);
  assert.match(workflowSource, /validate_release_inputs:/);
  assert.match(workflowSource, /build_frontend_dist:\s+needs:\s+validate_release_inputs/);
  assert.match(workflowSource, /git rev-parse/);
  assert.match(workflowSource, /CodexManagerLocal/);
});

test("local Tauri override and macOS first-launch helpers exist", () => {
  const configPath = path.join(repoDir, "apps", "src-tauri", "tauri.local.conf.json");
  const commandPath = path.join(
    repoDir,
    "assets",
    "macos-local",
    "Open CodexManagerLocal.command",
  );
  const readmePath = path.join(
    repoDir,
    "assets",
    "macos-local",
    "README-macOS-first-launch-local.txt",
  );

  assert.ok(fs.existsSync(configPath), "local Tauri config should exist");
  assert.ok(fs.existsSync(commandPath), "macOS helper should exist");
  assert.ok(fs.existsSync(readmePath), "macOS first-launch guide should exist");

  const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
  assert.equal(config.productName, "CodexManagerLocal");
  assert.equal(config.identifier, "com.codexmanager.local");
  assert.equal(config.app.windows[0].title, "CodexManager Local");
});

test("local macOS DMG packaging inherits version from the base Tauri config", () => {
  const localConfig = JSON.parse(
    readRepoFile("apps", "src-tauri", "tauri.local.conf.json"),
  );
  const script = readRepoFile(
    "scripts",
    "release",
    "rebuild-macos-dmg.sh",
  );

  assert.equal(localConfig.version, undefined);
  assert.match(script, /payload\.get\("version"\)/);
  assert.match(script, /with_name\("tauri\.conf\.json"\)/);
  assert.match(script, /version.*missing/i);
  assert.doesNotMatch(script, /\r/);
});
