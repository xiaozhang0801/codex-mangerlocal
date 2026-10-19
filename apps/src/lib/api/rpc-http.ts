import type { RequestOptions, ResponseReader } from "../utils/request";
import { requestWasNotSent, throwIfAborted } from "../utils/request";
import { getAppErrorMessage, unwrapRpcPayload } from "./transport-errors";
import {
  rpcAllowsReplay,
  rpcRequestOptions,
  rpcResultUnknown,
} from "./rpc-retry-policy";

export type JsonRpcFetcher = <T>(
  url: string,
  init: RequestInit,
  options: RequestOptions,
  readResponse: ResponseReader<T>
) => Promise<T>;

export function buildJsonRpcRequestBody(
  method: string,
  params: Record<string, unknown> = {}
): string {
  return JSON.stringify({
    jsonrpc: "2.0",
    id: Date.now(),
    method,
    params,
  });
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

async function readRpcHttpErrorPayload(response: Response): Promise<unknown> {
  if (typeof response.text === "function") {
    const raw = await response.text();
    const trimmed = raw.trim();
    if (!trimmed) {
      return null;
    }
    try {
      return JSON.parse(trimmed) as unknown;
    } catch {
      return trimmed;
    }
  }

  if (typeof response.json === "function") {
    try {
      return (await response.json()) as unknown;
    } catch (error) {
      if (error instanceof SyntaxError) return null;
      throw error;
    }
  }

  return null;
}

function buildRpcHttpErrorMessage(
  status: number,
  payload: unknown,
  headers?: Pick<Headers, "get">
): string {
  const detail = getAppErrorMessage(payload, "").trim();
  const errorCode =
    headers?.get("X-CodexManager-Error-Code")?.trim() ||
    asRecord(payload)?.errorCode?.toString().trim() ||
    "";
  const traceId = headers?.get("X-CodexManager-Trace-Id")?.trim() || "";

  let message = detail
    ? `RPC 请求失败（HTTP ${status}）：${detail}`
    : `RPC 请求失败（HTTP ${status}）`;
  const meta = [
    errorCode ? `code=${errorCode}` : "",
    traceId ? `trace=${traceId}` : "",
  ].filter(Boolean);
  if (meta.length) {
    message += ` [${meta.join(" ")}]`;
  }
  return message;
}

function isWebAuthRequiredPayload(payload: unknown): boolean {
  const record = asRecord(payload);
  return record?.error === "web_auth_required";
}

function redirectToWebLoginIfBrowser(): void {
  if (typeof window === "undefined") {
    return;
  }
  const currentPath =
    `${window.location.pathname || "/"}${window.location.search || ""}` || "/";
  const loginUrl = `/__login?next=${encodeURIComponent(currentPath)}`;
  window.location.replace(loginUrl);
}

export async function postJsonRpc<T>(
  fetcher: JsonRpcFetcher,
  url: string,
  rpcMethod: string,
  params: Record<string, unknown> = {},
  options: RequestOptions = {}
): Promise<T> {
  // An already-cancelled request is known not to have been sent.
  throwIfAborted(options.signal);
  const allowsReplay = rpcAllowsReplay(rpcMethod);
  const body = buildJsonRpcRequestBody(rpcMethod, params);
  let decoded: { response: Response; payload: unknown };
  try {
    decoded = await fetcher(
      url,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body,
        // 307/308 would otherwise replay the POST inside fetch itself.
        redirect: "error",
      },
      rpcRequestOptions(rpcMethod, options),
      async (response) => {
        const payload = response.ok
          ? (await response.json()) as unknown
          : await readRpcHttpErrorPayload(response);
        if (response.ok) {
          const record = asRecord(payload);
          if (!record || !("result" in record || "error" in record)) {
            throw new Error("Unexpected RPC response (missing result/error)");
          }
        }
        return { response, payload };
      }
    );
    // Readers only decode. Never redirect or publish a late result after cancel.
    throwIfAborted(options.signal);
  } catch (error) {
    // Browser fetch does not expose whether a connection error happened before
    // or after the server committed. Never infer safe replay from its message.
    if (!allowsReplay && !requestWasNotSent(error)) {
      throw rpcResultUnknown(rpcMethod, getAppErrorMessage(error), error);
    }
    throw error;
  }

  const { response, payload } = decoded;
  if (!response.ok) {
    if (response.status === 401 && isWebAuthRequiredPayload(payload)) {
      redirectToWebLoginIfBrowser();
    }
    const message = buildRpcHttpErrorMessage(
      response.status,
      payload,
      response.headers
    );
    throw allowsReplay ? new Error(message) : rpcResultUnknown(rpcMethod, message);
  }

  // A complete business error is definitive; do not label it as a lost result.
  return unwrapRpcPayload<T>(payload);
}
