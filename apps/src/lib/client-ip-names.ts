import ipaddr from "ipaddr.js";

export type ClientIpNames = Record<string, string>;
export const MAX_CLIENT_IP_NAMES = 5000;
export const MAX_PERSONNEL_FILE_BYTES = 2 * 1024 * 1024;

export function canonicalClientIp(value: string): string | null {
  const text = value.trim();
  // Reject shorthand/octal IPv4 and scoped addresses to match Rust IpAddr.
  if (!text || text.includes("%") || (!text.includes(":") && !ipaddr.IPv4.isValidFourPartDecimal(text))) {
    return null;
  }
  if (text.includes(":") && text.includes(".") && !ipaddr.IPv4.isValidFourPartDecimal(text.slice(text.lastIndexOf(":") + 1))) return null;
  try {
    return ipaddr.process(text).toString();
  } catch {
    return null;
  }
}

function addBinding(names: ClientIpNames, ip: string, value: unknown) {
  const key = canonicalClientIp(ip);
  if (!key) throw new Error("IP 地址无效");
  if (typeof value !== "string") throw new Error("姓名无效");
  const name = value.trim();
  if (!name || [...name].length > 100 || /[\p{Cc}]/u.test(value)) {
    throw new Error("姓名必须为 1–100 个字符，且不能包含控制字符");
  }
  if (names[key] && names[key] !== name) throw new Error("同一 IP 对应的姓名冲突");
  const added = !Object.hasOwn(names, key);
  names[key] = name;
  return added;
}

export function normalizeClientIpNames(value: unknown): ClientIpNames {
  if (!value || typeof value !== "object" || Array.isArray(value)) return {};
  const entries = Object.entries(value);
  if (entries.length > MAX_CLIENT_IP_NAMES) return {};
  const names: ClientIpNames = {};
  try {
    for (const [ip, name] of entries) addBinding(names, ip, name);
    return names;
  } catch {
    return {};
  }
}

export interface ClientIpNameImport {
  names: ClientIpNames;
  skipped: number;
}

export class ClientIpNameImportError extends Error {
  constructor(message: string, public readonly row: number) {
    super(message);
  }
}

export function parseClientIpNameRows(rows: readonly (readonly unknown[])[]): ClientIpNameImport {
  const headerText = (value: unknown) => String(value ?? "").replace(/\s/g, "").toLowerCase();
  const isIpHeader = (value: unknown) => ["ip", "ipv4", "ipv6", "ip地址", "ipv4地址(ip)", "ipv4地址（ip）"].includes(headerText(value));
  const headerIndex = rows.findIndex(row => row.some(cell => headerText(cell) === "姓名") && row.some(isIpHeader));
  if (headerIndex < 0) throw new Error("未找到姓名和 IP 表头");
  const header = rows[headerIndex];
  const nameColumn = header.findIndex(cell => headerText(cell) === "姓名");
  const ipColumn = header.findIndex(isIpHeader);
  const names: ClientIpNames = {};
  let skipped = 0;
  let count = 0;
  for (let index = headerIndex + 1; index < rows.length; index++) {
    const row = rows[index];
    if (row.every(cell => !String(cell ?? "").trim())) continue;
    const name = row[nameColumn];
    const ip = String(row[ipColumn] ?? "").trim().replace(/[ \t]*\.[ \t]*/g, ".");
    if (!String(name ?? "").trim() || !ip) {
      skipped++;
      continue;
    }
    try {
      if (addBinding(names, ip, name)) count++;
    } catch (error) {
      throw new ClientIpNameImportError((error as Error).message, index + 1);
    }
    if (count > MAX_CLIENT_IP_NAMES) throw new Error("绑定数量不能超过 5000");
  }
  if (!Object.keys(names).length) throw new Error("没有可导入的有效绑定");
  return { names, skipped };
}

export function formatClientIpName(ip: string, names: ClientIpNames, unbound: string): string {
  const key = canonicalClientIp(ip);
  return `${ip}（${(key && names[key]) || unbound}）`;
}

export async function readClientIpNameFile(file: File): Promise<ClientIpNameImport> {
  if (!/\.xlsx$/i.test(file.name)) throw new Error("请选择 .xlsx 文件");
  if (file.size > MAX_PERSONNEL_FILE_BYTES) throw new Error("人员表不能超过 2 MiB");
  // The worker-free export works under the desktop's existing strict CSP.
  const { readSheet } = await import("read-excel-file/universal");
  let rows;
  try {
    rows = await readSheet(file, { trim: false });
  } catch {
    throw new Error("无法读取 Excel 文件，请检查文件是否损坏或加密");
  }
  return parseClientIpNameRows(rows);
}
