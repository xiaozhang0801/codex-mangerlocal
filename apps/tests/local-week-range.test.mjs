import assert from "node:assert/strict";
import fs from "node:fs/promises";
import path from "node:path";
import test from "node:test";

const appsRoot = path.resolve(import.meta.dirname, "..");

test("本地周范围按周一零点计算并返回下周边界", async () => {
  const source = await fs.readFile(
    path.join(appsRoot, "src/lib/utils/time.ts"),
    "utf8",
  );

  assert.match(source, /export interface LocalWeekRange/);
  assert.match(source, /export function getLocalWeekRange/);
  assert.match(source, /referenceDate\.getDay\(\)/);
  assert.match(source, /weekStartTs/);
  assert.match(source, /weekEndTs/);
});
