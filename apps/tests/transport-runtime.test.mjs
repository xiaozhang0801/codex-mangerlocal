import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import { setImmediate as nextTurn } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const probeTimeoutMs = 80;

async function loadRuntimeModule({ retries = 0 } = {}) {
  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "codexmanager-transport-runtime-"));
  const sourcePaths = {
    runtime: path.join(appsRoot, "src", "lib", "api", "transport-runtime.ts"),
    request: path.join(appsRoot, "src", "lib", "utils", "request.ts"),
    capabilities: path.join(appsRoot, "src", "lib", "runtime", "runtime-capabilities.ts"),
  };

  try {
    for (const [name, sourcePath] of Object.entries(sourcePaths)) {
      let source = await fs.readFile(sourcePath, "utf8");
      if (name === "runtime") {
        // Only test timing/configuration is injected: use the actual request
        // helper and reader, but bound attempts to 80 ms instead of waiting
        // through the production 2.5-second deadline. Timeout cases disable
        // retries; the HTTP failure case keeps both production retries enabled.
        // Fresh temporary modules per test also isolate the capability cache.
        for (const [before, after] of [
          ["timeoutMs: 2500,", `timeoutMs: ${probeTimeoutMs},`],
          ["retries: 2,", `retries: ${retries},`],
          ["process.env.NEXT_PUBLIC_CODEXMANAGER_RPC_BASE_URL", '""'],
        ]) {
          assert.ok(source.includes(before), `Runtime test injection no longer matches: ${before}`);
          source = source.replace(before, after);
        }
      }

      const compiled = ts.transpileModule(source, {
        compilerOptions: {
          module: ts.ModuleKind.ES2022,
          target: ts.ScriptTarget.ES2022,
        },
        fileName: sourcePath,
      });
      const output = compiled.outputText
        .replaceAll('"@tauri-apps/api/core"', '"./tauri.mjs"')
        .replaceAll('"../utils/request"', '"./request.mjs"')
        .replaceAll('"../runtime/runtime-capabilities"', '"./capabilities.mjs"');
      await fs.writeFile(path.join(tempDir, `${name}.mjs`), output, "utf8");
    }
    await fs.writeFile(path.join(tempDir, "tauri.mjs"), "export const isTauri = () => false;\n", "utf8");
    return await import(pathToFileURL(path.join(tempDir, "runtime.mjs")).href);
  } finally {
    await fs.rm(tempDir, { recursive: true, force: true });
  }
}

function deferred() {
  let resolve;
  const promise = new Promise((resolvePromise) => {
    resolve = resolvePromise;
  });
  return { promise, resolve };
}

async function within(promise, timeoutMs = 2_000) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error("Runtime probe test exceeded its bound")), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function trackedResponse(signal, { status = 200, abortBody = true } = {}) {
  const readStarted = deferred();
  const readCompleted = deferred();
  let streamController;
  let ended = false;
  let jsonCalls = 0;
  let cancelCount = 0;
  let abortCount = 0;
  const abort = () => {
    abortCount += 1;
    if (abortBody && !ended) {
      ended = true;
      streamController.error(new DOMException("Runtime response body aborted", "AbortError"));
    }
  };
  const detach = () => signal.removeEventListener("abort", abort);
  const stream = new ReadableStream({
    start(controller) {
      streamController = controller;
    },
    cancel() {
      cancelCount += 1;
      ended = true;
      detach();
    },
  });
  signal.addEventListener("abort", abort, { once: true });
  const response = new Response(stream, { status });
  const readJson = response.json.bind(response);
  response.json = () => {
    jsonCalls += 1;
    readStarted.resolve();
    const pending = readJson();
    pending.then(() => readCompleted.resolve(), () => readCompleted.resolve());
    return pending;
  };

  return {
    response,
    readStarted: readStarted.promise,
    readCompleted: readCompleted.promise,
    get jsonCalls() { return jsonCalls; },
    get cancelCount() { return cancelCount; },
    get abortCount() { return abortCount; },
    enqueue(value) {
      streamController.enqueue(new TextEncoder().encode(value));
    },
    close() {
      detach();
      if (!ended) {
        ended = true;
        streamController.close();
      }
    },
  };
}

async function withBrowserFetch(fetcher, run) {
  const originalFetch = globalThis.fetch;
  const hadWindow = Object.hasOwn(globalThis, "window");
  const originalWindow = globalThis.window;
  globalThis.fetch = fetcher;
  globalThis.window = { location: { origin: "https://codexmanager.test" } };
  try {
    return await run();
  } finally {
    globalThis.fetch = originalFetch;
    if (hadWindow) globalThis.window = originalWindow;
    else delete globalThis.window;
  }
}

test("runtime probe deadline covers a stalled body after immediate headers", async () => {
  const runtime = await loadRuntimeModule();
  let body;
  let calls = 0;
  await withBrowserFetch(async (_url, init) => {
    calls += 1;
    body = trackedResponse(init.signal);
    return body.response;
  }, async () => {
    try {
      const result = await within(runtime.loadRuntimeCapabilities());
      assert.equal(result.mode, "unsupported-web");
      assert.equal(runtime.getCachedRuntimeCapabilities(), result);
      assert.equal(body.jsonCalls, 1);
      assert.equal(body.abortCount, 1);
      assert.equal(calls, 1);
      assert.equal(await runtime.loadRuntimeCapabilities(), result);
      assert.equal(calls, 1);
    } finally {
      body?.close();
    }
  });
});

test("late runtime body completion cannot overwrite the deadline fallback cache", async () => {
  const runtime = await loadRuntimeModule();
  let body;
  const bodyCreated = deferred();
  let calls = 0;
  await withBrowserFetch(async (_url, init) => {
    calls += 1;
    // Deliberately ignore the signal in this synthetic body. The helper must
    // reject on its own deadline and keep the eventual JSON result private.
    body = trackedResponse(init.signal, { abortBody: false });
    bodyCreated.resolve();
    return body.response;
  }, async () => {
    try {
      const pending = runtime.loadRuntimeCapabilities();
      await within(bodyCreated.promise);
      await within(body.readStarted);
      assert.equal(runtime.getCachedRuntimeCapabilities(), null);
      const fallback = await within(pending);
      assert.equal(fallback.mode, "unsupported-web");
      assert.equal(body.abortCount, 1);

      body.enqueue(JSON.stringify({ mode: "web-gateway", rpcBaseUrl: "/late/rpc" }));
      body.close();
      await within(body.readCompleted);
      await nextTurn();
      assert.equal(runtime.getCachedRuntimeCapabilities(), fallback);
      assert.equal(await runtime.loadRuntimeCapabilities(), fallback);
      assert.equal(calls, 1);
    } finally {
      body?.close();
    }
  });
});

test("runtime probe releases an unsuccessful body without decoding or retrying it", async () => {
  const runtime = await loadRuntimeModule({ retries: 2 });
  let body;
  let calls = 0;
  await withBrowserFetch(async (_url, init) => {
    calls += 1;
    body = trackedResponse(init.signal, { status: 503, abortBody: false });
    return body.response;
  }, async () => {
    try {
      const result = await within(runtime.loadRuntimeCapabilities());
      assert.equal(result.mode, "unsupported-web");
      assert.equal(runtime.getCachedRuntimeCapabilities(), result);
      assert.equal(body.jsonCalls, 0);
      assert.equal(body.abortCount, 1);
      assert.equal(body.cancelCount, 1);
      assert.equal(calls, 1);
    } finally {
      body?.close();
    }
  });
});

test("runtime probe caches only complete parsed capabilities and reuses the shared request", async () => {
  const runtime = await loadRuntimeModule();
  let body;
  const bodyCreated = deferred();
  let calls = 0;
  await withBrowserFetch(async (url, init) => {
    calls += 1;
    assert.equal(new URL(url).pathname, "/api/runtime");
    assert.equal(init.method, "GET");
    assert.equal(init.cache, "no-store");
    body = trackedResponse(init.signal);
    bodyCreated.resolve();
    return body.response;
  }, async () => {
    try {
      const first = runtime.loadRuntimeCapabilities();
      const concurrent = runtime.loadRuntimeCapabilities();
      await within(bodyCreated.promise);
      await within(body.readStarted);
      assert.equal(runtime.getCachedRuntimeCapabilities(), null);
      body.enqueue('{"mode":"web-gateway","rpcBaseUrl":"/managed/');
      assert.equal(runtime.getCachedRuntimeCapabilities(), null);
      body.enqueue('rpc/","canSelfUpdate":true}');
      body.close();
      const result = await within(first);
      assert.equal(result.mode, "web-gateway");
      assert.equal(result.rpcBaseUrl, "/managed/rpc");
      assert.equal(result.canSelfUpdate, true);
      assert.equal(await concurrent, result);
      assert.equal(runtime.getCachedRuntimeCapabilities(), result);
      assert.equal(await runtime.loadRuntimeCapabilities(), result);
      assert.equal(body.jsonCalls, 1);
      assert.equal(body.abortCount, 0);
      assert.equal(calls, 1);
    } finally {
      body?.close();
    }
  });
});
