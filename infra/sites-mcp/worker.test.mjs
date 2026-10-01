import test from "node:test";
import assert from "node:assert/strict";
import worker from "./src/worker.mjs";

const endpoint = "https://mcp.example.test/mcp";
const env = { MCP_UPSTREAM_URL: "https://room.example.test/mcp" };

test("streams protocol content while isolating both authentication boundaries", async t => {
  const payload = '{"jsonrpc":"2.0","id":2,"method":"tools/list"}';
  t.mock.method(globalThis, "fetch", async (url, init) => {
    assert.equal(url.toString(), env.MCP_UPSTREAM_URL);
    assert.equal(init.redirect, "manual");
    assert.equal(init.headers.get("mcp-protocol-version"), "2025-03-26");
    assert.equal(init.headers.get("mcp-session-id"), "protocol-session");
    for (const name of ["authorization", "cookie", "origin", "host", "oai-sites-authorization",
      "oai-authenticated-user-email", "x-forwarded-host", "x-agentsassemble-proxy-token"]) {
      assert.equal(init.headers.get(name), null, name);
    }
    assert.equal(await new Response(init.body).text(), payload);
    return new Response("native protocol result", { status: 400, headers: {
      "content-type": "application/json", "mcp-session-id": "protocol-session",
      "set-cookie": "upstream-private-cookie", location: "https://foreign.example.test",
    } });
  });
  const response = await worker.fetch(new Request(endpoint, {
    method: "POST", body: payload, headers: {
      accept: "application/json, text/event-stream", "content-type": "application/json",
      "mcp-protocol-version": "2025-03-26", "mcp-session-id": "protocol-session",
      origin: "https://mcp.example.test", authorization: "Bearer sites-test-only",
      cookie: "sites-test-only", "oai-sites-authorization": "sites-test-only",
      "oai-authenticated-user-email": "fixture@example.test",
      host: "attacker.example.test", "x-forwarded-host": "attacker.example.test",
      "x-agentsassemble-proxy-token": "attacker-test-only",
    },
  }), env);
  assert.equal(response.status, 400);
  assert.equal(await response.text(), "native protocol result");
  assert.equal(response.headers.get("cache-control"), "private, no-store");
  assert.equal(response.headers.get("mcp-session-id"), "protocol-session");
  assert.equal(response.headers.get("set-cookie"), null);
  assert.equal(response.headers.get("location"), null);
});

test("rejects foreign origins, caller-selected destinations and unsupported methods before I/O", async t => {
  t.mock.method(globalThis, "fetch", () => assert.fail("network access forbidden"));
  for (const [url, options, status] of [
    [endpoint, { method: "POST", headers: { origin: "https://foreign.example.test" } }, 403],
    [endpoint + "?upstream=https://foreign.example.test", { method: "POST" }, 404],
    [endpoint + "/other", { method: "POST" }, 404],
    [endpoint, { method: "PUT" }, 405],
  ]) assert.equal((await worker.fetch(new Request(url, options), env)).status, status);
  for (const upstream of [undefined, "http://room.example.test/mcp", "https://127.0.0.1/mcp",
    "https://[::1]/mcp", "https://room.local/mcp", "https://room.example.test:8443/mcp",
    "https://user:secret@room.example.test/mcp", "https://room.example.test/mcp?token=test",
    "https://room.example.test/mcp#test", "https://room.example.test/other"]) {
    assert.equal((await worker.fetch(new Request(endpoint), { MCP_UPSTREAM_URL: upstream })).status, 503);
  }
});

test("refuses redirects and reports transport failure without leaking exception content", async t => {
  t.mock.method(globalThis, "fetch", async () => new Response(null, {
    status: 307, headers: { location: "https://foreign.example.test" },
  }));
  const redirected = await worker.fetch(new Request(endpoint), env);
  assert.equal(redirected.status, 502);
  assert.equal((await redirected.json()).error.code, "mcp_upstream_redirect_rejected");
  t.mock.restoreAll();
  t.mock.method(globalThis, "fetch", async () => { throw new Error("private payload fixture"); });
  const failed = await worker.fetch(new Request(endpoint), env);
  assert.equal(failed.status, 502);
  assert.equal(await failed.text(), '{"error":{"code":"mcp_upstream_unavailable"}}');
});

test("passes cancellation and streams a long response without buffering or a timer", async t => {
  const controller = new AbortController();
  const request = new Request(endpoint, { signal: controller.signal });
  let finish;
  const stream = new ReadableStream({ start(control) { finish = control; } });
  t.mock.method(globalThis, "fetch", async (_url, init) => {
    assert.equal(init.signal, request.signal);
    return new Response(stream, { headers: { "content-type": "text/event-stream" } });
  });
  const response = await worker.fetch(request, env);
  const reader = response.body.getReader();
  const pending = reader.read();
  finish.enqueue(new TextEncoder().encode("data: protocol result\n\n"));
  assert.equal(new TextDecoder().decode((await pending).value), "data: protocol result\n\n");
  finish.close();
  assert.equal((await reader.read()).done, true);
  controller.abort();
  assert.equal(request.signal.aborted, true);
});
