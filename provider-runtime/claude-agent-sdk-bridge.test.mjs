import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import test from "node:test";
import { fileURLToPath } from "node:url";

const bridge = fileURLToPath(new URL("./claude-agent-sdk-bridge.mjs", import.meta.url));
const fakeSdk = fileURLToPath(new URL("./claude-agent-sdk-fake.mjs", import.meta.url));

function start(mode) {
  const child = spawn(process.execPath, [bridge, fakeSdk, "/fixture/claude", mode], {
    stdio: ["pipe", "pipe", "pipe"],
  });
  const lines = createInterface({ input: child.stdout });
  const iterator = lines[Symbol.asyncIterator]();
  return {
    child,
    async next() {
      const line = await iterator.next();
      assert.equal(line.done, false);
      return JSON.parse(line.value);
    },
  };
}

function closed(child) {
  return new Promise((resolve) => child.once("close", (code) => resolve(code)));
}

test("catalog emits only exact installed model authority", async () => {
  const runtime = start("catalog");
  assert.deepEqual(await runtime.next(), {
    type: "catalog",
    models: [
      {
        id: "claude-sonnet-5",
        label: "Claude Sonnet 5",
        efforts: ["low", "medium", "high", "xhigh"],
        fast: true,
      },
    ],
  });
  assert.equal(await closed(runtime.child), 0);
});

test("usage projects structured SDK limits without session or transcript metadata", async () => {
  const runtime = start("usage");
  const exit = closed(runtime.child);
  assert.deepEqual(await runtime.next(), {
    type: "usage", rate_limits_available: true,
    rate_limits: { five_hour: { utilization: 12.5, resets_at: null }, seven_day: null },
  });
  assert.equal(await exit, 0);
});

test("session correlates one SDK result and closes explicitly", async () => {
  const runtime = start("session");
  runtime.child.stdin.write(
    `${JSON.stringify({
      type: "initialize",
      workspace: "/fixture/workspace",
      model: "claude-sonnet-5",
      reasoning_effort: "high",
      service_tier: "fast",
      permission_mode: "meeting_read_only",
      resume_session_id: "",
      room_portal: { url: "http://127.0.0.1:43210/mcp", bearer_token: "fixture-token" },
    })}\n`,
  );
  const ready = await runtime.next();
  assert.equal(ready.type, "ready");
  assert.match(ready.session_id, /^[0-9a-f-]{36}$/);

  runtime.child.stdin.write(`${JSON.stringify({ type: "turn", turn_id: "turn-1", input: "hello" })}\n`);
  assert.deepEqual(await runtime.next(), {
    type: "turn_result",
    turn_id: "turn-1",
    provider_turn_id: "sdk-result-id",
    session_id: ready.session_id,
    content: "fixture response",
  });
  runtime.child.stdin.write(`${JSON.stringify({ type: "shutdown" })}\n`);
  assert.deepEqual(await runtime.next(), { type: "stopped" });
  assert.equal(await closed(runtime.child), 0);
});

test("session rejects a non-UUID durable identity", async () => {
  const runtime = start("session");
  const exit = closed(runtime.child);
  runtime.child.stdin.write(
    `${JSON.stringify({
      type: "initialize",
      workspace: "/fixture/workspace",
      model: "claude-sonnet-5",
      reasoning_effort: "high",
      service_tier: "default",
      permission_mode: "meeting_read_only",
      resume_session_id: "legacy-session-alias",
      room_portal: { url: "http://127.0.0.1:43210/mcp", bearer_token: "fixture-token" },
    })}\n`,
  );
  assert.deepEqual(await runtime.next(), { type: "fatal", code: "claude_sdk_bridge_failed" });
  assert.equal(await exit, 1);
});
