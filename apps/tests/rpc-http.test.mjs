import assert from "node:assert/strict";
import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import test from "node:test";
import http, { createServer } from "node:http";
import { setTimeout as delay } from "node:timers/promises";
import { pathToFileURL } from "node:url";
import ts from "../node_modules/typescript/lib/typescript.js";

const appsRoot = path.resolve(import.meta.dirname, "..");
const rpcHttpSourcePath = path.join(
  appsRoot,
  "src",
  "lib",
  "api",
  "rpc-http.ts"
);
const transportErrorsSourcePath = path.join(
  appsRoot,
  "src",
  "lib",
  "api",
  "transport-errors.ts"
);

async function loadRpcHttpModule() {
  const [rpcHttpSource, transportErrorsSource, retryPolicySource, policyJson, requestSource] = await Promise.all([
    fs.readFile(rpcHttpSourcePath, "utf8"),
    fs.readFile(transportErrorsSourcePath, "utf8"),
    fs.readFile(path.join(appsRoot, "src/lib/api/rpc-retry-policy.ts"), "utf8"),
    fs.readFile(path.join(appsRoot, "src/lib/api/rpc-retry-policy.json"), "utf8"),
    fs.readFile(path.join(appsRoot, "src/lib/utils/request.ts"), "utf8"),
  ]);

  const rpcHttpCompiled = ts.transpileModule(rpcHttpSource, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: rpcHttpSourcePath,
  });
  const transportErrorsCompiled = ts.transpileModule(transportErrorsSource, {
    compilerOptions: {
      module: ts.ModuleKind.ES2022,
      target: ts.ScriptTarget.ES2022,
    },
    fileName: transportErrorsSourcePath,
  });

  const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), "codexmanager-rpc-http-"));
  test.after(() => fs.rm(tempDir, { recursive: true, force: true }));
  const rpcHttpFile = path.join(tempDir, "rpc-http.mjs");
  const transportErrorsFile = path.join(tempDir, "transport-errors.mjs");

  await fs.writeFile(
    rpcHttpFile,
    rpcHttpCompiled.outputText.replaceAll('"./rpc-retry-policy"', '"./rpc-retry-policy.mjs"').replaceAll('"../utils/request"', '"./request.mjs"').replace(
      /from "\.\/transport-errors"/g,
      'from "./transport-errors.mjs"'
    ),
    "utf8"
  );
  await fs.writeFile(transportErrorsFile, transportErrorsCompiled.outputText, "utf8");
  const compile = (source) => ts.transpileModule(source, {
    compilerOptions: { module: ts.ModuleKind.ES2022, target: ts.ScriptTarget.ES2022 },
  }).outputText;
  await fs.writeFile(path.join(tempDir, "rpc-retry-policy.mjs"),
    compile(retryPolicySource).replaceAll('"./rpc-retry-policy.json"', '"./policy-data.mjs"'));
  await fs.writeFile(path.join(tempDir, "policy-data.mjs"), `export default ${policyJson};`);
  await fs.writeFile(path.join(tempDir, "request.mjs"), compile(requestSource));
  return {
    ...await import(pathToFileURL(rpcHttpFile).href),
    ...await import(pathToFileURL(path.join(tempDir, "request.mjs")).href),
    ...await import(pathToFileURL(path.join(tempDir, "rpc-retry-policy.mjs")).href),
  };
}

const rpcHttp = await loadRpcHttpModule();
const request = rpcHttp;

function responseFetcher(response) {
  return async (_url, _init, _options, readResponse) => readResponse(response);
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
        timer = setTimeout(() => reject(new Error("RPC test exceeded its bound")), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

async function withBrowserFetch(fetcher, run) {
  const originalFetch = globalThis.fetch;
  const originalWindow = globalThis.window;
  const redirects = [];
  globalThis.fetch = fetcher;
  globalThis.window = {
    location: {
      pathname: "/settings",
      search: "?tab=general",
      replace(url) { redirects.push(url); },
    },
  };
  try {
    return await run(redirects);
  } finally {
    globalThis.fetch = originalFetch;
    if (originalWindow === undefined) delete globalThis.window;
    else globalThis.window = originalWindow;
  }
}

async function withStalledHttpBody(status, run) {
  let requests = 0;
  const sockets = new Set();
  const server = createServer((incoming, response) => {
    requests += 1;
    incoming.resume();
    response.writeHead(status, { "Content-Type": "application/json" });
    response.flushHeaders();
    // Deliberately leave a valid JSON prefix unfinished after sending headers.
    response.write(status === 200 ? '{"result":' : '{"error":');
  });
  server.on("connection", (socket) => {
    sockets.add(socket);
    socket.once("close", () => sockets.delete(socket));
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(0, "127.0.0.1", resolve);
  });
  try {
    await run({
      url: `http://127.0.0.1:${server.address().port}/api/rpc`,
      get requests() { return requests; },
    });
  } finally {
    const closed = new Promise((resolve, reject) => {
      server.close((error) => error ? reject(error) : resolve());
    });
    for (const socket of sockets) socket.destroy();
    await within(closed);
  }
}

test("buildJsonRpcRequestBody 生成标准 JSON-RPC envelope", () => {
  const payload = JSON.parse(
    rpcHttp.buildJsonRpcRequestBody("demo/method", { foo: "bar" })
  );

  assert.equal(payload.jsonrpc, "2.0");
  assert.equal(payload.method, "demo/method");
  assert.deepEqual(payload.params, { foo: "bar" });
  assert.equal(typeof payload.id, "number");
});

test("postJsonRpc 统一通过 fetcher 发送并解包结果", async () => {
  const calls = [];
  const result = await rpcHttp.postJsonRpc(
    async (url, init, options, readResponse) => {
      calls.push({ url, init, options });
      return readResponse({
        ok: true,
        status: 200,
        async json() {
          return { result: { ok: true, value: 7 } };
        },
      });
    },
    "/api/rpc",
    "demo/method",
    { foo: "bar" },
    { retries: 0 }
  );

  assert.equal(result.value, 7);
  assert.equal(calls.length, 1);
  assert.equal(calls[0].url, "/api/rpc");
  assert.equal(calls[0].options.retries, 0);
  assert.equal(calls[0].init.method, "POST");
  assert.equal(calls[0].init.headers["Content-Type"], "application/json");
});

test("postJsonRpc 对非 2xx 响应会带出服务端错误详情与链路标识", async () => {
  await assert.rejects(
    () =>
      rpcHttp.postJsonRpc(
        responseFetcher({
          ok: false,
          status: 503,
          headers: {
            get(name) {
              if (name === "X-CodexManager-Error-Code") return "upstream_timeout";
              if (name === "X-CodexManager-Trace-Id") return "trace-rpc-503";
              return null;
            },
          },
          async text() {
            return JSON.stringify({
              error: {
                message: "上游请求超时",
              },
            });
          },
        }),
        "/api/rpc",
        "demo/method"
      ),
    /HTTP 503.*上游请求超时.*upstream_timeout.*trace-rpc-503/
  );
});

test("postJsonRpc 遇到 Web 鉴权失效会跳转登录页", async () => {
  const originalWindow = globalThis.window;
  const redirects = [];
  globalThis.window = {
    location: {
      pathname: "/settings",
      search: "?tab=general",
      replace(url) {
        redirects.push(url);
      },
    },
  };

  try {
    await assert.rejects(
      () =>
        rpcHttp.postJsonRpc(
          responseFetcher({
            ok: false,
            status: 401,
            headers: { get: () => null },
            async text() {
              return JSON.stringify({ error: "web_auth_required" });
            },
          }),
          "/api/rpc",
          "app/settings/get"
        ),
      /HTTP 401/
    );
  } finally {
    if (originalWindow === undefined) {
      delete globalThis.window;
    } else {
      globalThis.window = originalWindow;
    }
  }

  assert.deepEqual(redirects, [
    "/__login?next=%2Fsettings%3Ftab%3Dgeneral",
  ]);
});

for (const status of [200, 401]) {
  test(`postJsonRpc deadline interrupts a real HTTP ${status} body after immediate headers`, async () => {
    await withStalledHttpBody(status, async (server) => {
      await withBrowserFetch(globalThis.fetch, async (redirects) => {
        let readStarted = false;
        const fetcher = (url, init, options, readResponse) => request.fetchWithRetry(
          url, init, options, (response) => {
            readStarted = true;
            return readResponse(response);
          },
        );
        await assert.rejects(within(rpcHttp.postJsonRpc(
          fetcher, server.url, "apikey/list", {}, { timeoutMs: 150, retries: 0 },
        )), { name: "TimeoutError" });
        assert.equal(readStarted, true);
        assert.equal(server.requests, 1);
        assert.deepEqual(redirects, []);
      });
    });
  });

  test(`postJsonRpc parent cancellation interrupts a real HTTP ${status} body without replay or redirect`, async () => {
    await withStalledHttpBody(status, async (server) => {
      await withBrowserFetch(globalThis.fetch, async (redirects) => {
        const parent = new AbortController();
        const reading = deferred();
        const fetcher = (url, init, options, readResponse) => request.fetchWithRetry(
          url, init, options, (response) => {
            reading.resolve();
            return readResponse(response);
          },
        );
        const pending = rpcHttp.postJsonRpc(
          fetcher, server.url, "apikey/list", {},
          { signal: parent.signal, timeoutMs: 1_000, retries: 2, retryDelayMs: 0 },
        );
        const rejected = assert.rejects(within(pending), { name: "AbortError" });
        await within(reading.promise);
        parent.abort();
        await rejected;
        assert.equal(server.requests, 1);
        assert.deepEqual(redirects, []);
      });
    });
  });
}

for (const status of [200, 401]) {
  test(`postJsonRpc ignores a noncooperative ${status === 200 ? "JSON" : "error text"} reader that resolves after cancellation`, async () => {
    const parent = new AbortController();
    const reading = deferred();
    const lateBody = deferred();
    let fetchCalls = 0;
    let jsonReads = 0;
    let textReads = 0;
    let bodyCancels = 0;
    let cache;
    const response = new Response(new ReadableStream({
      cancel() { bodyCancels += 1; },
    }), { status });
    response.json = () => {
      jsonReads += 1;
      reading.resolve();
      return lateBody.promise;
    };
    response.text = () => {
      textReads += 1;
      reading.resolve();
      return lateBody.promise;
    };
    await withBrowserFetch(async (_url, init) => {
      assert.equal(init.method, "POST");
      fetchCalls += 1;
      return fetchCalls === 1 ? response : Response.json({ result: { saved: true } });
    }, async (redirects) => {
      const pending = rpcHttp.postJsonRpc(
        request.fetchWithRetry, "/api/rpc", "apikey/list", {},
        { signal: parent.signal, retries: 2, retryDelayMs: 0, timeoutMs: 1_000 },
      ).then(async (value) => {
        cache = value;
        // Model a consumer that submits a dependent write after loading data.
        await rpcHttp.postJsonRpc(request.fetchWithRetry, "/api/rpc", "demo/write");
      });
      const rejected = assert.rejects(within(pending), { name: "AbortError" });
      await within(reading.promise);
      parent.abort();
      await rejected;
      lateBody.resolve(status === 200
        ? { result: { stale: true } }
        : JSON.stringify({ error: "web_auth_required" }));
      await delay(10);
      assert.equal(cache, undefined);
      assert.equal(fetchCalls, 1);
      assert.equal(bodyCancels, 1);
      assert.equal(jsonReads, status === 200 ? 1 : 0);
      assert.equal(textReads, status === 401 ? 1 : 0);
      assert.deepEqual(redirects, []);
    });
  });
}

for (const status of [200, 401]) {
  test(`postJsonRpc checks cancellation after a custom fetcher finishes reading HTTP ${status}`, async () => {
    const parent = new AbortController();
    let cache;
    let calls = 0;
    const response = Response.json(status === 200
      ? { result: { value: 7 } }
      : { error: "web_auth_required" }, { status });
    await withBrowserFetch(async () => {
      throw new Error("Custom fetcher should not call global fetch");
    }, async (redirects) => {
      const fetcher = async (_url, _init, _options, readResponse) => {
        calls += 1;
        const result = await readResponse(response);
        parent.abort();
        return result;
      };
      await assert.rejects(rpcHttp.postJsonRpc(
        fetcher, "/api/rpc", "apikey/list", {}, { signal: parent.signal },
      ).then((value) => { cache = value; }), { name: "AbortError" });
      assert.equal(calls, 1);
      assert.equal(cache, undefined);
      assert.deepEqual(redirects, []);
    });
  });
}

test("postJsonRpc rejects an already canceled caller before invoking a custom fetcher", async () => {
  const parent = new AbortController();
  parent.abort();
  let calls = 0;
  await assert.rejects(rpcHttp.postJsonRpc(
    async () => { calls += 1; throw new Error("unexpected fetcher call"); },
    "/api/rpc", "demo/write", {}, { signal: parent.signal },
  ), { name: "AbortError" });
  assert.equal(calls, 0);
});

for (const failure of [
  { name: "invalid JSON", body: "{invalid json", expected: { name: "SyntaxError" } },
  { name: "RPC error", body: JSON.stringify({ error: { code: -32000, message: "RPC rejected" } }), expected: /RPC rejected/ },
  { name: "business error", body: JSON.stringify({ result: { ok: false, error: "write rejected" } }), expected: /write rejected/ },
]) {
  test(`postJsonRpc does not replay HTTP 200 with ${failure.name}`, async () => {
    let fetchCalls = 0;
    await withBrowserFetch(async () => {
      fetchCalls += 1;
      return new Response(failure.body);
    }, async () => {
      await assert.rejects(within(rpcHttp.postJsonRpc(
        request.fetchWithRetry, "/api/rpc", "apikey/list", {}, { retries: 2, retryDelayMs: 0 },
      )), failure.expected);
      assert.equal(fetchCalls, 1);
    });
  });
}

for (const readError of [
  new DOMException("error body canceled", "AbortError"),
  new TypeError("error body connection lost"),
]) {
  test(`postJsonRpc preserves ${readError.name} from error text without a second JSON read`, async () => {
    let fetchCalls = 0;
    let textReads = 0;
    let jsonReads = 0;
    const response = new Response(null, { status: 401 });
    response.text = async () => { textReads += 1; throw readError; };
    response.json = async () => { jsonReads += 1; return { error: "web_auth_required" }; };
    await withBrowserFetch(async () => {
      fetchCalls += 1;
      return response;
    }, async (redirects) => {
      await assert.rejects(within(rpcHttp.postJsonRpc(
        request.fetchWithRetry, "/api/rpc", "apikey/list", {}, { retries: 2, retryDelayMs: 0 },
      )), (error) => error === readError);
      assert.equal(fetchCalls, 1);
      assert.equal(textReads, 1);
      assert.equal(jsonReads, 0);
      assert.deepEqual(redirects, []);
    });
  });
}

test("postJsonRpc preserves AbortError from a JSON-only error response", async () => {
  const canceled = new DOMException("JSON error body canceled", "AbortError");
  let jsonReads = 0;
  let fetchCalls = 0;
  await withBrowserFetch(async () => {
    fetchCalls += 1;
    return {
      ok: false,
      status: 401,
      body: null,
      headers: new Headers(),
      async json() { jsonReads += 1; throw canceled; },
    };
  }, async (redirects) => {
    await assert.rejects(within(rpcHttp.postJsonRpc(
      request.fetchWithRetry, "/api/rpc", "apikey/list", {}, { retries: 2, retryDelayMs: 0 },
    )), (error) => error === canceled);
    assert.equal(fetchCalls, 1);
    assert.equal(jsonReads, 1);
    assert.deepEqual(redirects, []);
  });
});

test("postJsonRpc reports malformed error text without trying to consume the body as JSON again", async () => {
  let jsonReads = 0;
  const response = new Response("upstream returned a plain error", { status: 400 });
  response.json = async () => { jsonReads += 1; throw new Error("body already consumed"); };
  await withBrowserFetch(async () => response, async () => {
    await assert.rejects(rpcHttp.postJsonRpc(
      request.fetchWithRetry, "/api/rpc", "apikey/list", {}, { retries: 0 },
    ), /HTTP 400.*upstream returned a plain error/);
    assert.equal(jsonReads, 0);
  });
});

async function withRpcServer(handler, run) {
  const server = http.createServer((req, res) => {
    let body = "";
    req.setEncoding("utf8");
    req.on("data", (chunk) => { body += chunk; });
    req.on("end", () => handler(JSON.parse(body), req, res));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    await run(`http://127.0.0.1:${server.address().port}/rpc`);
  } finally {
    const closed = new Promise((resolve, reject) => server.close((err) => err ? reject(err) : resolve()));
    server.closeAllConnections();
    await closed;
  }
}

const quickRetry = { retries: 3, retryDelayMs: 0, timeoutMs: 500, shouldRetry: () => true, shouldRetryStatus: () => true };

test("RPC policy uses exact audited names and defaults unknown operations to no replay", () => {
  assert.equal(rpcHttp.getRpcRetryKind("apikey/list"), "read-only");
  assert.equal(rpcHttp.getRpcRetryKind("apikey/enable"), "idempotent-write");
  for (const method of ["apikey/create", "account/usage/resetCredit/consume", "future/get", "account/read", "initialize", "account/usage/resetCredits", "appSettings/get", "authorContent/get"]) {
    assert.equal(rpcHttp.getRpcRetryKind(method), "non-idempotent-write", method);
  }
});

test("committed API key creation with lost response executes once despite caller retry overrides", async () => {
  const createdKeys = [];
  await withRpcServer((body, req) => {
    assert.equal(body.method, "apikey/create");
    createdKeys.push({ id: `key-${createdKeys.length + 1}` }); // Commit before losing the response.
    req.socket.destroy();
  }, async (url) => {
    await assert.rejects(rpcHttp.postJsonRpc(rpcHttp.fetchWithRetry, url, "apikey/create", { name: "test" }, quickRetry), /rpc_result_unknown:.*结果未知.*刷新核实/);
    assert.equal(createdKeys.length, 1);
  });
});

for (const failure of ["http-error", "invalid-json", "invalid-envelope", "truncated-body"]) {
  test(`non-idempotent writes are not replayed after ${failure}`, async () => {
    let commits = 0;
    await withRpcServer((_body, req, res) => {
      commits += 1;
      if (failure === "http-error") { res.writeHead(503); res.end("service response lost"); }
      if (failure === "invalid-json") { res.end("{"); }
      if (failure === "invalid-envelope") { res.end("{}"); }
      if (failure === "truncated-body") {
        res.writeHead(200, { "Content-Length": "1000" });
        res.flushHeaders();
        res.write('{"result":');
        setImmediate(() => req.socket.destroy());
      }
    }, async (url) => {
      await assert.rejects(rpcHttp.postJsonRpc(rpcHttp.fetchWithRetry, url, "apikey/create", {}, quickRetry), /rpc_result_unknown:/);
      assert.equal(commits, 1);
    });
  });
}

test("reset credit consume and unknown RPCs cannot opt into replay", async () => {
  for (const method of ["account/usage/resetCredit/consume", "future/create"]) {
    let commits = 0;
    await withRpcServer((_body, req) => { commits += 1; req.socket.destroy(); }, async (url) => {
      await assert.rejects(rpcHttp.postJsonRpc(rpcHttp.fetchWithRetry, url, method, {}, quickRetry), /rpc_result_unknown:/);
      assert.equal(commits, 1, method);
    });
  }
});

test("read-only and explicitly idempotent RPCs still retry transient failures", async () => {
  for (const method of ["apikey/list", "apikey/disable"]) {
    let attempts = 0;
    await withRpcServer((_body, req, res) => {
      attempts += 1;
      if (attempts === 1) { req.socket.destroy(); return; }
      if (attempts === 2) { res.writeHead(503); res.end("temporary"); return; }
      res.end(JSON.stringify({ result: { ok: true } }));
    }, async (url) => {
      assert.deepEqual(await rpcHttp.postJsonRpc(rpcHttp.fetchWithRetry, url, method, {}, { ...quickRetry, shouldRetryStatus: undefined }), { ok: true });
      assert.equal(attempts, 3);
    });
  }
});

test("read-only retry count remains bounded at three retries by default", async () => {
  let attempts = 0;
  await withRpcServer((_body, _req, res) => { attempts += 1; res.writeHead(503); res.end("temporary"); }, async (url) => {
    await assert.rejects(rpcHttp.postJsonRpc(rpcHttp.fetchWithRetry, url, "apikey/list", {}, { retryDelayMs: 0 }), /HTTP 503/);
    assert.equal(attempts, 4);
  });
});

test("definitive RPC business errors preserve their message without claiming unknown outcome", async () => {
  await assert.rejects(rpcHttp.postJsonRpc(responseFetcher(new Response(JSON.stringify({ result: { ok: false, error: "invalid key name" } }))), "/rpc", "apikey/create"), { message: "invalid key name" });
});

test("pre-cancelled writes never start fetch or claim unknown outcome", async () => {
  const controller = new AbortController();
  controller.abort();
  await assert.rejects(rpcHttp.postJsonRpc(async () => { assert.fail("fetch must not start"); }, "/rpc", "apikey/create", {}, { signal: controller.signal }), { name: "AbortError" });
});

test("serialization failures stay definitive before any fetch", async () => {
  const params = {};
  params.circular = params;
  await assert.rejects(rpcHttp.postJsonRpc(async () => { assert.fail("fetch must not start"); }, "/rpc", "apikey/create", params), (error) => error instanceof TypeError && !error.message.includes("rpc_result_unknown:"));
});

test("fetch cannot replay a committed POST through 307/308 redirects", async () => {
  for (const status of [307, 308]) {
    let commits = 0;
    await withRpcServer((_body, req, res) => {
      commits += 1;
      if (req.url === "/rpc") {
        res.writeHead(status, { Location: "/redirected" });
        res.end();
      } else {
        res.end(JSON.stringify({ result: { id: "duplicate" } }));
      }
    }, async (url) => {
      await assert.rejects(rpcHttp.postJsonRpc(rpcHttp.fetchWithRetry, url, "apikey/create", {}, quickRetry), /rpc_result_unknown:/);
      assert.equal(commits, 1);
    });
  }
});

for (const status of [200, 401]) {
  for (const stop of ["deadline", "caller-cancel"]) {
    test(`committed write with HTTP ${status} stalled body: ${stop} cannot bypass replay cap`, async () => {
      let commits = 0;
      await withRpcServer((_body, _req, res) => {
        commits += 1;
        res.writeHead(status, { "Content-Type": "application/json" });
        res.flushHeaders();
        res.write(status === 200 ? '{"result":' : '{"error":');
      }, async (url) => {
        await withBrowserFetch(globalThis.fetch, async (redirects) => {
          const controller = new AbortController();
          const reading = deferred();
          const fetcher = (url, init, options, readResponse) => request.fetchWithRetry(
            url, init, options, (response) => {
              reading.resolve();
              return readResponse(response);
            },
          );
          const pending = rpcHttp.postJsonRpc(fetcher, url, "apikey/create", {}, {
            ...quickRetry, timeoutMs: stop === "deadline" ? 150 : 1000,
            signal: controller.signal,
          });
          const rejected = assert.rejects(within(pending), (error) => {
            assert.match(error.message, /rpc_result_unknown:.*刷新核实/);
            assert.equal(error.name, stop === "deadline" ? "TimeoutError" : "AbortError");
            assert.equal(error.cause.name, error.name);
            return true;
          });
          await within(reading.promise);
          if (stop === "caller-cancel") controller.abort();
          await rejected;
          assert.equal(commits, 1);
          assert.deepEqual(redirects, []);
        });
      });
    });
  }
}

test("read-only stalled body is aborted before starting its successful retry", async () => {
  let sends = 0;
  let previousSignal;
  let previousBody;
  let released = 0;
  await withBrowserFetch(async (_url, init) => {
    sends += 1;
    if (sends === 1) {
      previousSignal = init.signal;
      previousBody = new Response(new ReadableStream({ cancel() { released += 1; } }));
      // A non-cooperative decoder also must not delay cancellation/retry.
      previousBody.json = () => new Promise(() => {});
      return previousBody;
    }
    assert.equal(previousSignal.aborted, true);
    assert.equal(released, 1);
    return Response.json({ result: { count: 7 } });
  }, async () => {
    assert.deepEqual(await within(rpcHttp.postJsonRpc(
      request.fetchWithRetry, "/rpc", "apikey/list", {},
      { timeoutMs: 40, retries: 1, retryDelayMs: 0 },
    )), { count: 7 });
    assert.equal(sends, 2);
  });
});

test("real HTTP read-only retry aborts its previous response before the second send", async () => {
  let sends = 0;
  const events = [];
  const realFetch = globalThis.fetch;
  await withRpcServer((_body, _req, res) => {
    sends += 1;
    if (sends === 1) {
      res.writeHead(200, { "Content-Type": "application/json" });
      res.flushHeaders();
      res.write('{"result":');
    } else {
      res.end(JSON.stringify({ result: { recovered: true } }));
    }
  }, async (url) => {
    let fetchCalls = 0;
    await withBrowserFetch((url, init) => {
      fetchCalls += 1;
      events.push(`send-${fetchCalls}`);
      if (fetchCalls === 1) {
        init.signal.addEventListener("abort", () => events.push("abort-first"), { once: true });
      } else {
        assert.deepEqual(events, ["send-1", "abort-first", "send-2"]);
      }
      return realFetch(url, init);
    }, async () => {
      assert.deepEqual(await within(rpcHttp.postJsonRpc(
        request.fetchWithRetry, url, "apikey/list", {},
        { timeoutMs: 150, retries: 1, retryDelayMs: 0 },
      )), { recovered: true });
      assert.equal(sends, 2);
      assert.equal(fetchCalls, 2);
    });
  });
});

for (const failure of ["queued-cancel", "queued-deadline"]) {
  test(`write rejected before actual fetch stays definitive: ${failure}`, async () => {
    let sends = 0;
    await withBrowserFetch(async () => {
      sends += 1;
      return Response.json({ result: { id: "must-not-be-created" } });
    }, async () => {
      const controller = new AbortController();
      const pending = rpcHttp.postJsonRpc(request.fetchWithRetry, "/rpc", "apikey/create", {}, {
        ...quickRetry, timeoutMs: 10, signal: controller.signal,
      });
      if (failure === "queued-cancel") {
        controller.abort();
      } else {
        // Microtasks run before an overdue timer. The actual send must check
        // the monotonic deadline instead of relying on the timer firing first.
        const started = performance.now();
        while (performance.now() - started < 40) { /* model busy decoding */ }
      }
      await assert.rejects(within(pending), (error) => {
        assert.equal(error.name, failure === "queued-cancel" ? "AbortError" : "TimeoutError");
        assert.doesNotMatch(error.message, /rpc_result_unknown:/);
        return true;
      });
      assert.equal(sends, 0);
    });
  });
}
