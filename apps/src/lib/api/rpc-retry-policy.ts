import policy from "./rpc-retry-policy.json";
import type { RequestOptions } from "../utils/request";

export type RpcRetryKind = "read-only" | "idempotent-write" | "non-idempotent-write";

const readOnlyMethods = new Set(policy.readOnlyMethods);
const idempotentWriteMethods = new Set(policy.idempotentWriteMethods);

// Shared with the native RPC client. Review both storage dispatch paths before
// adding a method: a get/read name can still refresh tokens or migrate files.
export function getRpcRetryKind(method: string): RpcRetryKind {
  if (readOnlyMethods.has(method)) return "read-only";
  if (idempotentWriteMethods.has(method)) return "idempotent-write";
  return "non-idempotent-write";
}

export function rpcAllowsReplay(method: string): boolean {
  return getRpcRetryKind(method) !== "non-idempotent-write";
}

export function rpcRequestOptions(
  method: string,
  options: RequestOptions,
): RequestOptions {
  // Apply the safety cap after caller options, including at the outer IPC layer.
  return rpcAllowsReplay(method) ? options : { ...options, retries: 0 };
}

export function rpcResultUnknown(method: string, detail: string, cause?: unknown): Error {
  const error = new Error(
    `rpc_result_unknown: ${method} 请求结果未知，服务端可能已完成，已停止自动重试。请先刷新核实后再决定是否重试。${detail ? ` (${detail})` : ""}`,
    { cause },
  );
  if (cause instanceof Error && (cause.name === "AbortError" || cause.name === "TimeoutError")) {
    error.name = cause.name;
  }
  return error;
}
