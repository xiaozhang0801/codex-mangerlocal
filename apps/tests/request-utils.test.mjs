import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const sourcePath = path.join(appsRoot, "src", "lib", "utils", "request.ts");

async function loadRequestModule() {
  const source = await fs.readFile(sourcePath, "utf8");
  const compiled = ts.transpileModule(source, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: sourcePath,
  });

  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "codexmanager-request-"));
  const tempFile = path.join(tempDir, "request.mjs");
  await fs.writeFile(tempFile, compiled.outputText, "utf8");
  try {
    return await import(pathToFileURL(tempFile).href);
  } finally {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
}

const request = await loadRequestModule();

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

async function within(promise, timeoutMs = 2_000) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("Request test exceeded its bound")), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

// Synthetic Responses do not inherit the fetch signal, so connect it to the
// stream explicitly to model a browser interrupting an in-progress body read.
function trackedResponse({ signal, status = 200, cancelNeverSettles = false } = {}) {
  let streamController;
  let cancelCount = 0;
  let abortCount = 0;
  const abort = () => {
    abortCount += 1;
    streamController.error(new DOMException("Body read aborted", "AbortError"));
  };
  const detach = () => signal?.removeEventListener("abort", abort);
  const stream = new ReadableStream({
    start(controller) {
      streamController = controller;
    },
    cancel() {
      cancelCount += 1;
      detach();
      if (cancelNeverSettles) {
        return new Promise(() => {});
      }
    },
  });
  signal?.addEventListener("abort", abort, { once: true });
  return {
    response: new Response(stream, { status }),
    get cancelCount() { return cancelCount; },
    get abortCount() { return abortCount; },
    enqueue(text) {
      streamController.enqueue(new TextEncoder().encode(text));
    },
    close() {
      detach();
      streamController.close();
    },
    detach,
  };
}

async function withFetch(fetcher, run) {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = fetcher;
  try {
    return await run();
  } finally {
    globalThis.fetch = originalFetch;
  }
}

test("fetchWithRetry rejects before fetch when parent signal is already aborted", async () => {
  const controller = new AbortController();
  controller.abort();
  let called = false;
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => {
    called = true;
    return new Response(null, { status: 200 });
  };

  try {
    await assert.rejects(
      request.fetchWithRetry("https://example.test/rpc", {}, { signal: controller.signal }),
      { name: "AbortError" },
    );
    assert.equal(called, false);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test("runWithControl aborts retry delay without starting another attempt", async () => {
  const controller = new AbortController();
  let attempts = 0;
  const pending = request.runWithControl(
    async () => {
      attempts += 1;
      throw new Error("temporary failure");
    },
    {
      retries: 2,
      retryDelayMs: 1_000,
      signal: controller.signal,
    },
  );

  setTimeout(() => controller.abort(), 10);
  await assert.rejects(pending, { name: "AbortError" });
  assert.equal(attempts, 1);
});

test("fetchWithRetry rejects before fetch when init.signal is already aborted", async () => {
  const controller = new AbortController();
  controller.abort();
  let attempts = 0;
  await withFetch(async () => {
    attempts += 1;
    return new Response("{}");
  }, async () => {
    await assert.rejects(
      request.fetchWithRetry("https://example.test/rpc", { signal: controller.signal }, {}, (response) => response.json()),
      { name: "AbortError" },
    );
    assert.equal(attempts, 0);
  });
});

test("fetchWithRetry deadline covers a stalled body after immediate headers", async () => {
  let body;
  let attempts = 0;
  await withFetch(async (_url, init) => {
    attempts += 1;
    body = trackedResponse({ signal: init.signal });
    return body.response;
  }, async () => {
    await assert.rejects(
      within(request.fetchWithRetry("https://example.test/rpc", {}, { timeoutMs: 25, retries: 0 }, (response) => response.json())),
      { name: "TimeoutError" },
    );
    assert.equal(attempts, 1);
    assert.equal(body.abortCount, 1);
  });
});

test("fetchWithRetry can retry a body deadline and return the next complete JSON", async () => {
  let stalledBody;
  let attempts = 0;
  await withFetch(async (_url, init) => {
    attempts += 1;
    if (attempts === 1) {
      stalledBody = trackedResponse({ signal: init.signal });
      return stalledBody.response;
    }
    assert.equal(stalledBody.abortCount, 1);
    return Response.json({ attempt: attempts });
  }, async () => {
    const result = await within(request.fetchWithRetry(
      "https://example.test/rpc", {}, { timeoutMs: 30, retries: 1, retryDelayMs: 0 }, (response) => response.json(),
    ));
    assert.deepEqual(result, { attempt: 2 });
    assert.equal(attempts, 2);
  });
});

for (const signalSource of ["init", "options"]) {
  for (const status of [200, 400]) {
    test(`fetchWithRetry honors ${signalSource}.signal during ${status === 200 ? "success JSON" : "error text"} body reading`, async () => {
      const initParent = new AbortController();
      const optionsParent = new AbortController();
      const bodyStarted = deferred();
      let body;
      let attempts = 0;
      await withFetch(async (_url, init) => {
        attempts += 1;
        body = trackedResponse({ signal: init.signal, status });
        return body.response;
      }, async () => {
        const pending = request.fetchWithRetry(
          "https://example.test/rpc",
          { signal: initParent.signal },
          { signal: optionsParent.signal, retries: 2, retryDelayMs: 0, timeoutMs: 1_000 },
          (response) => {
            bodyStarted.resolve();
            return response.ok ? response.json() : response.text();
          },
        );
        const rejection = assert.rejects(within(pending), { name: "AbortError" });
        await within(bodyStarted.promise);
        (signalSource === "init" ? initParent : optionsParent).abort();
        await rejection;
        assert.equal(attempts, 1);
        assert.equal(body.abortCount, 1);
      });
    });
  }
}

test("fetchWithRetry cancellation interrupts backoff and does not issue the next request", async () => {
  const parent = new AbortController();
  const discarded = trackedResponse({ status: 503 });
  let attempts = 0;
  let abortTimer;
  await withFetch(async () => {
    attempts += 1;
    abortTimer = setTimeout(() => parent.abort(), 20);
    return discarded.response;
  }, async () => {
    try {
      await assert.rejects(within(request.fetchWithRetry(
        "https://example.test/rpc", {},
        { signal: parent.signal, retries: 2, retryDelayMs: 1_000 },
        (response) => response.json(),
      )), { name: "AbortError" });
      assert.equal(attempts, 1);
      assert.equal(discarded.cancelCount, 1);
    } finally {
      clearTimeout(abortTimer);
    }
  });
});

test("fetchWithRetry cancellation also prevents a retry when the backoff is zero", async () => {
  const parent = new AbortController();
  const discarded = trackedResponse({ status: 503 });
  let attempts = 0;
  await withFetch(async () => {
    attempts += 1;
    return discarded.response;
  }, async () => {
    await assert.rejects(within(request.fetchWithRetry(
      "https://example.test/rpc", { signal: parent.signal },
      {
        retries: 2,
        retryDelayMs: 0,
        shouldRetryStatus() {
          parent.abort();
          return true;
        },
      },
      (response) => response.json(),
    )), { name: "AbortError" });
    assert.equal(attempts, 1);
    assert.equal(discarded.cancelCount, 1);
  });
});

for (const status of [429, 503]) {
  test(`fetchWithRetry releases the discarded ${status} body before starting its retry`, async () => {
    // A stream's cancel hook can itself stall; disposal must not stall retry.
    const discarded = trackedResponse({ status, cancelNeverSettles: true });
    let attempts = 0;
    await withFetch(async () => {
      attempts += 1;
      if (attempts === 1) return discarded.response;
      assert.equal(discarded.cancelCount, 1);
      return Response.json({ ok: true });
    }, async () => {
      const result = await within(request.fetchWithRetry(
        "https://example.test/rpc", {}, { retries: 1, retryDelayMs: 0 }, (response) => response.json(),
      ));
      assert.deepEqual(result, { ok: true });
      assert.equal(attempts, 2);
      assert.equal(discarded.cancelCount, 1);
    });
  });
}

test("fetchWithRetry reads the final non-retryable error body within its lifecycle", async () => {
  let attempts = 0;
  await withFetch(async () => {
    attempts += 1;
    return new Response("request was rejected", { status: 400 });
  }, async () => {
    const result = await request.fetchWithRetry(
      "https://example.test/rpc", {}, { retries: 2 }, (response) => response.text(),
    );
    assert.equal(result, "request was rejected");
    assert.equal(attempts, 1);
  });
});

test("fetchWithRetry does not replay a response with invalid JSON", async () => {
  let attempts = 0;
  await withFetch(async () => {
    attempts += 1;
    return new Response("{invalid json");
  }, async () => {
    await assert.rejects(
      within(request.fetchWithRetry("https://example.test/rpc", {}, { retries: 2, retryDelayMs: 0 }, (response) => response.json())),
      { name: "SyntaxError" },
    );
    assert.equal(attempts, 1);
  });
});

test("fetchWithRetry reads a normal large JSON body in multiple chunks", async () => {
  const payload = { items: Array.from({ length: 10_000 }, (_, index) => ({ index, text: "x".repeat(120) })) };
  const serialized = JSON.stringify(payload);
  let body;
  let completionTimer;
  await withFetch(async (_url, init) => {
    body = trackedResponse({ signal: init.signal });
    const midpoint = Math.floor(serialized.length / 2);
    body.enqueue(serialized.slice(0, midpoint));
    completionTimer = setTimeout(() => {
      body.enqueue(serialized.slice(midpoint));
      body.close();
    }, 10);
    return body.response;
  }, async () => {
    try {
      const result = await within(request.fetchWithRetry(
        "https://example.test/rpc", {}, { timeoutMs: 1_500, retries: 0 }, (response) => response.json(),
      ));
      assert.deepEqual(result, payload);
      assert.equal(body.abortCount, 0);
      assert.equal(body.cancelCount, 0);
    } finally {
      clearTimeout(completionTimer);
      body.detach();
    }
  });
});

test("fetchWithRetry cancels a late response after a noncooperative fetch times out", async () => {
  const lateFetch = deferred();
  const lateBody = trackedResponse();
  let attempts = 0;
  let readCount = 0;
  let cache;
  let followupWrites = 0;
  await withFetch(async () => {
    attempts += 1;
    return lateFetch.promise;
  }, async () => {
    const pending = request.fetchWithRetry(
      "https://example.test/rpc", {}, { timeoutMs: 25, retries: 0 }, (response) => {
        readCount += 1;
        return response.json();
      },
    ).then((value) => {
      cache = value;
      followupWrites += 1;
    });
    await assert.rejects(within(pending), { name: "TimeoutError" });
    lateFetch.resolve(lateBody.response);
    await delay(10);
    assert.equal(lateBody.cancelCount, 1);
    assert.equal(readCount, 0);
    assert.equal(cache, undefined);
    assert.equal(followupWrites, 0);
    assert.equal(attempts, 1);
  });
});

test("fetchWithRetry does not publish a late reader result after its timeout", async () => {
  const lateRead = deferred();
  const body = trackedResponse();
  let cache;
  let followupWrites = 0;
  await withFetch(async () => body.response, async () => {
    const pending = request.fetchWithRetry(
      "https://example.test/rpc", {}, { timeoutMs: 25, retries: 0 }, () => lateRead.promise,
    ).then((value) => {
      cache = value;
      followupWrites += 1;
    });
    await assert.rejects(within(pending), { name: "TimeoutError" });
    lateRead.resolve({ stale: true });
    await delay(10);
    assert.equal(body.cancelCount, 1);
    assert.equal(cache, undefined);
    assert.equal(followupWrites, 0);
  });
});

test("fetchWithRetry rejects a reader that finishes synchronous decoding after the deadline", async () => {
  await withFetch(async () => Response.json({ ok: true }), async () => {
    await assert.rejects(within(request.fetchWithRetry(
      "https://example.test/rpc", {}, { timeoutMs: 10, retries: 0 }, async (response) => {
        const value = await response.json();
        // A synchronous decoder cannot yield to the timeout callback. The
        // completed value still must not escape an already elapsed deadline.
        const started = performance.now();
        while (performance.now() - started < 35) {
          JSON.parse('{"decode":"work"}');
        }
        return value;
      },
    )), { name: "TimeoutError" });
  });
});

test("fetchWithRetry raw Response mode leaves a continuing SSE body to its caller", async () => {
  let body;
  let requestSignal;
  await withFetch(async (_url, init) => {
    requestSignal = init.signal;
    body = trackedResponse({ signal: init.signal });
    body.enqueue("data: first\n\n");
    return body.response;
  }, async () => {
    const response = await request.fetchWithRetry(
      "https://example.test/events", {}, { timeoutMs: 20, retries: 0 },
    );
    const reader = response.body.getReader();
    try {
      assert.equal(new TextDecoder().decode((await reader.read()).value), "data: first\n\n");
      await delay(50);
      assert.equal(requestSignal.aborted, false);
      body.enqueue("data: second\n\n");
      assert.equal(new TextDecoder().decode((await reader.read()).value), "data: second\n\n");
    } finally {
      await reader.cancel();
      reader.releaseLock();
      body.detach();
    }
    assert.equal(body.cancelCount, 1);
  });
});
