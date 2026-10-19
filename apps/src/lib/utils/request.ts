export interface RequestOptions {
  signal?: AbortSignal;
  timeoutMs?: number;
  retries?: number;
  retryDelayMs?: number;
  maxRetryDelayMs?: number;
  shouldRetry?: (error: unknown) => boolean;
  shouldRetryStatus?: (status: number) => boolean;
}

// Readers must only consume/decode data. Publish results after the controlled
// request resolves, since an aborted reader may finish late in a custom fetcher.
export type ResponseReader<T> = (response: Response) => Promise<T>;

// Only this controller can prove that fetch was never invoked. Do not infer it
// from browser network errors, which can happen after the server has committed.
const failuresBeforeSend = new WeakSet<Error>();

export function requestWasNotSent(error: unknown): boolean {
  return error instanceof Error && failuresBeforeSend.has(error);
}

function createAbortError(): Error {
  if (typeof DOMException !== "undefined") {
    return new DOMException("Request aborted", "AbortError");
  }
  const error = new Error("Request aborted");
  error.name = "AbortError";
  return error;
}

function createTimeoutError(): Error {
  const error = new Error("Request deadline exceeded");
  error.name = "TimeoutError";
  return error;
}

export function throwIfAborted(signal: AbortSignal | undefined): void {
  if (signal?.aborted) {
    throw createAbortError();
  }
}

function waitForDelay(ms: number, signal?: AbortSignal): Promise<void> {
  throwIfAborted(signal);
  if (ms <= 0) {
    return Promise.resolve();
  }
  return new Promise((resolve, reject) => {
    const id = setTimeout(() => {
      signal?.removeEventListener("abort", abort);
      resolve();
    }, ms);
    const abort = () => {
      clearTimeout(id);
      signal?.removeEventListener("abort", abort);
      reject(createAbortError());
    };
    signal?.addEventListener("abort", abort, { once: true });
  });
}

function runWithAbortSignal<T>(
  fn: () => Promise<T>,
  signal?: AbortSignal,
): Promise<T> {
  throwIfAborted(signal);
  if (!signal) {
    return fn();
  }
  return new Promise((resolve, reject) => {
    const abort = () => {
      signal.removeEventListener("abort", abort);
      reject(createAbortError());
    };
    signal.addEventListener("abort", abort, { once: true });
    Promise.resolve()
      .then(() => {
        throwIfAborted(signal);
        return fn();
      })
      .then((value) => {
        signal.removeEventListener("abort", abort);
        if (signal.aborted) {
          reject(createAbortError());
        } else {
          resolve(value);
        }
      }, (error: unknown) => {
        signal.removeEventListener("abort", abort);
        reject(error);
      });
  });
}

function releaseResponseBody(response: Response): void {
  // Abort the fetch controller as well when discarding an attempt: cancel()
  // alone cannot release a body currently locked by a reader. Never let a
  // non-cooperative stream's cancellation delay the next attempt.
  try {
    void response.body?.cancel().catch(() => {});
  } catch {
    // A locked/already consumed stream is released by the fetch controller.
  }
}

/**
 * 函数 `fetchWithRetry`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - url: 参数 url
 * - init?: 参数 init?
 * - options: 参数 options
 *
 * # 返回
 * 返回函数执行结果
 */
export function fetchWithRetry(
  url: string,
  init?: RequestInit,
  options?: RequestOptions
): Promise<Response>;
export function fetchWithRetry<T>(
  url: string,
  init: RequestInit | undefined,
  options: RequestOptions | undefined,
  readResponse: ResponseReader<T>
): Promise<T>;
/**
 * With a reader, each attempt's deadline covers headers, body and decoding.
 * Without one, ownership transfers at headers (for streaming callers); the
 * caller then owns body consumption and cancellation.
 */
export async function fetchWithRetry<T>(
  url: string,
  init?: RequestInit,
  options: RequestOptions = {},
  readResponse?: ResponseReader<T>
): Promise<Response | T> {
  const {
    timeoutMs = 10000,
    retries = 3,
    retryDelayMs = 200,
    maxRetryDelayMs = 3000,
    shouldRetryStatus = (status) => status >= 500 || status === 429,
  } = options;

  const parent = new AbortController();
  const parentSignals = new Set([options.signal, init?.signal]);
  const abortParent = () => parent.abort();
  for (const signal of parentSignals) {
    signal?.addEventListener("abort", abortParent, { once: true });
    if (signal?.aborted) parent.abort();
  }

  let fetchStarted = false;
  try {
    let lastError: unknown;
    for (let i = 0; i <= retries; i++) {
      throwIfAborted(parent.signal);
      const controller = new AbortController();
      const deadline = performance.now() + timeoutMs;
      const id = setTimeout(() => controller.abort(), timeoutMs);
      const abortFromParent = () => controller.abort();
      parent.signal.addEventListener("abort", abortFromParent, { once: true });
      const checkDeadline = () => {
        // Timers cannot interrupt synchronous JSON decoding. Reject an overdue
        // result before consumers can publish it, even if the timer hasn't run.
        if (performance.now() >= deadline) controller.abort();
        throwIfAborted(controller.signal);
      };
      let response: Response | undefined;
      let transferResponse = false;
      let readingResponse = false;

      try {
        response = await runWithAbortSignal(async () => {
          // An overdue timer may not have run yet after synchronous work. Never
          // send a queued write once this attempt's deadline has already passed.
          checkDeadline();
          fetchStarted = true;
          const fetched = await fetch(url, { ...init, signal: controller.signal });
          response = fetched;
          if (controller.signal.aborted) {
            releaseResponseBody(fetched);
            throw createAbortError();
          }
          return fetched;
        }, controller.signal);
        checkDeadline();

        if (response.ok || !shouldRetryStatus(response.status) || i === retries) {
          readingResponse = true;
          const currentResponse = response;
          const result = readResponse
            ? await runWithAbortSignal(() => readResponse(currentResponse), controller.signal)
            : response;
          checkDeadline();
          transferResponse = !readResponse;
          return result;
        }
      } catch (err: unknown) {
        throwIfAborted(parent.signal);
        if (performance.now() >= deadline) controller.abort();
        // Parent cancellation was checked above. An aborted attempt here means
        // its own deadline expired; keep that distinct from a user's cancel.
        lastError = controller.signal.aborted ? createTimeoutError() : err;
        // Body timeouts may retry, but decoder/application failures must not
        // replay a request merely because decoding its response failed.
        if (i === retries || (readingResponse && !controller.signal.aborted)) {
          throw lastError;
        }
      } finally {
        clearTimeout(id);
        parent.signal.removeEventListener("abort", abortFromParent);
        if (!transferResponse) {
          controller.abort();
          if (response) releaseResponseBody(response);
        }
      }

      const delay = Math.min(retryDelayMs * Math.pow(2, i), maxRetryDelayMs);
      await waitForDelay(delay, parent.signal);
    }
    throw lastError || new Error("Fetch failed after retries");
  } catch (error) {
    if (!fetchStarted && error instanceof Error) failuresBeforeSend.add(error);
    throw error;
  } finally {
    for (const signal of parentSignals) {
      signal?.removeEventListener("abort", abortParent);
    }
  }
}

/**
 * 函数 `runWithControl`
 *
 * 作者: gaohongshun
 *
 * 时间: 2026-04-02
 *
 * # 参数
 * - fn: 参数 fn
 * - options: 参数 options
 *
 * # 返回
 * 返回函数执行结果
 */
export async function runWithControl<T>(
  fn: () => Promise<T>,
  options: RequestOptions = {}
): Promise<T> {
  const {
    retries = 0,
    retryDelayMs = 200,
    maxRetryDelayMs = 3000,
    shouldRetry = () => true,
  } = options;

  let lastError: unknown;
  for (let i = 0; i <= retries; i++) {
    try {
      return await runWithAbortSignal(fn, options.signal);
    } catch (err: unknown) {
      lastError = err;
      if (err instanceof Error && err.name === "AbortError") {
        throw err;
      }
      if (i === retries || !shouldRetry(err)) {
        throw err;
      }
    }
    const delay = Math.min(retryDelayMs * Math.pow(2, i), maxRetryDelayMs);
    await waitForDelay(delay, options.signal);
  }
  throw lastError;
}
