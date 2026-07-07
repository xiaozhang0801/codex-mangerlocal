import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const pageSectionsSource = readFileSync("src/app/logs/page-sections.tsx", "utf8");
const pageCellsSource = readFileSync("src/app/logs/page-cells.tsx", "utf8");

assert.match(pageSectionsSource, /客户端 IP/);
assert.match(pageSectionsSource, /ClientIpCell/);
assert.match(pageCellsSource, /function ClientIpCell/);
