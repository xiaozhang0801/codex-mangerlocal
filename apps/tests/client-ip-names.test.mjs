import assert from "node:assert/strict";
import fs from "node:fs/promises";
import { createRequire } from "node:module";
import vm from "node:vm";
import test from "node:test";
import ts from "../node_modules/typescript/lib/typescript.js";
import { personnelWorkbook } from "./support/personnel-workbook.cjs";

const require = createRequire(import.meta.url);
async function loadBindings() {
  const path = new URL("../src/lib/client-ip-names.ts", import.meta.url);
  assert.ok(await fs.access(path).then(() => true, () => false), "IP name parsing/display helper must exist");
  const source = await fs.readFile(path, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true },
  });
  const exports = {};
  vm.runInNewContext(compiled.outputText, { exports, require });
  return exports;
}
const plain = (value) => JSON.parse(JSON.stringify(value));

test("personnel import finds headers after titles and skips incomplete/blank rows", async () => {
  const { parseClientIpNameRows } = await loadBindings();
  const result = parseClientIpNameRows([
    ["Personnel"], [], ["姓名", "IPv4 地址(IP)"],
    [" Alice ", " 192. 0. 2.1 "], ["Bob", "2001:0db8::1"],
    ["Missing", null], [null, "192.0.2.2"], [],
  ]);
  assert.deepEqual(plain(result), { names: { "192.0.2.1": "Alice", "2001:db8::1": "Bob" }, skipped: 2 });
});

test("duplicate identical bindings collapse; conflicting canonical IPs reject", async () => {
  const { parseClientIpNameRows } = await loadBindings();
  const rows = [["IP", "姓名"], ["192.0.2.1", "Alice"], ["::ffff:192.0.2.1", "Alice"]];
  assert.equal(Object.keys(parseClientIpNameRows(rows).names).length, 1);
  rows[2][1] = "Bob";
  assert.throws(() => parseClientIpNameRows(rows), /IP.*冲突/);
});

test("invalid IP, malformed name, missing headers and empty imports reject", async () => {
  const { parseClientIpNameRows } = await loadBindings();
  for (const ip of ["999.1.1.1", "192.0.2.1:80", "127.1", "010.0.0.1", "::ffff:010.0.0.1", "https://example.com", "fe80::1%eth0"]) {
    assert.throws(() => parseClientIpNameRows([["姓名", "IP"], ["Alice", ip]]), /IP/);
  }
  assert.throws(() => parseClientIpNameRows([["姓名", "IP"], ["a\u0000b", "192.0.2.1"]]), /姓名/);
  assert.throws(() => parseClientIpNameRows([["姓名", "IP"], ["\tAlice", "192.0.2.1"]]), /姓名/);
  assert.throws(() => parseClientIpNameRows([["姓名", "IP"], ["a".repeat(101), "192.0.2.1"]]), /姓名/);
  assert.throws(() => parseClientIpNameRows([["owner", "address"]]), /表头/);
  assert.throws(() => parseClientIpNameRows([["姓名", "IP"], ["Alice", ""]]), /有效/);
});

test("mapping size is bounded and invalid settings normalize to an empty map", async () => {
  const { parseClientIpNameRows, normalizeClientIpNames } = await loadBindings();
  const rows = [["姓名", "IP"], ...Array.from({ length: 5001 }, (_, i) => ["Alice", `2001:db8::${(i + 1).toString(16)}`])];
  assert.throws(() => parseClientIpNameRows(rows), /5000/);
  for (const value of [null, [], "bad", { "bad": "Alice" }, { "192.0.2.1": 123 }]) {
    assert.deepEqual(plain(normalizeClientIpNames(value)), {});
  }
  assert.deepEqual(plain(normalizeClientIpNames({ "::ffff:192.0.2.1": " Alice " })), { "192.0.2.1": "Alice" });
});

test("display preserves original IP and appends current name or unbound", async () => {
  const { formatClientIpName } = await loadBindings();
  const names = { "192.0.2.1": "Alice", "2001:db8::1": "Bob" };
  assert.equal(formatClientIpName("192.0.2.1", names, "未绑定"), "192.0.2.1（Alice）");
  assert.equal(formatClientIpName("::ffff:192.0.2.1", names, "未绑定"), "::ffff:192.0.2.1（Alice）");
  assert.equal(formatClientIpName("2001:0db8::1", names, "未绑定"), "2001:0db8::1（Bob）");
  assert.equal(formatClientIpName("192.0.2.2", names, "未绑定"), "192.0.2.2（未绑定）");
  assert.equal(formatClientIpName("192.0.2.1", {}, "Unbound"), "192.0.2.1（Unbound）");
});

test("all requested display surfaces share the reactive IP name component", async () => {
  const dashboard = await fs.readFile(new URL("../src/app/page.tsx", import.meta.url), "utf8");
  const logs = await fs.readFile(new URL("../src/app/logs/page-cells.tsx", import.meta.url), "utf8");
  assert.match(dashboard, /<ClientIpLabel ip=\{group\.clientIp\}/);
  assert.match(dashboard, /<ClientIpLabel ip=\{item\.clientIp\}/);
  assert.match(logs, /<ClientIpLabel ip=\{clientIp\}/);
});

test("real XLSX bytes parse; wrong format, corrupt files and oversize files reject", async () => {
  const { readClientIpNameFile } = await loadBindings();
  const bytes = personnelWorkbook([["姓名", "IPv4 地址(IP)"], [" Alice ", "192. 0. 2.1"]]);
  const result = await readClientIpNameFile(new File([bytes], "people.xlsx"));
  assert.deepEqual(plain(result.names), { "192.0.2.1": "Alice" });
  const controls = personnelWorkbook([["姓名", "IP"], ["\tAlice", "192.0.2.1"]]);
  await assert.rejects(readClientIpNameFile(new File([controls], "controls.xlsx")), /姓名/);
  await assert.rejects(readClientIpNameFile(new File([bytes], "people.csv")), /xlsx/);
  await assert.rejects(readClientIpNameFile(new File(["bad"], "people.xlsx")), /无法读取/);
  await assert.rejects(readClientIpNameFile(new File([new Uint8Array(2 * 1024 * 1024 + 1)], "people.xlsx")), /2 MiB/);
});

test("desktop settings exposes an import preview with explicit replace and clear actions", async () => {
  const settings = await fs.readFile(new URL("../src/app/settings/page.tsx", import.meta.url), "utf8");
  assert.ok(settings.includes("<ClientIpNamesSettings"), "personnel import must be reachable from desktop settings");
});
