import assert from "node:assert/strict";
import { once } from "node:events";
import { createInterface } from "node:readline";
import test from "node:test";
import { NativeDelivery } from "./claude-native-delivery.mjs";

const hook = {
  type: "control_request", request_id: "native-1",
  request: {
    subtype: "hook_callback", callback_id: "callback-1", tool_use_id: "tool-1",
    input: { hook_event_name: "PermissionRequest", tool_name: "Write", tool_input: { file_path: "private" } },
  },
};
const response = {
  type: "control_response",
  response: { subtype: "success", request_id: "native-1", response: { behavior: "allow" } },
};

function child(delivery, code) {
  return delivery.spawn({
    command: process.execPath, args: ["--input-type=module", "-e", code],
    env: {}, signal: new AbortController().signal,
  });
}

test("native request correlation and exact flushed response retain process custody", async () => {
  const delivery = new NativeDelivery();
  const process = child(delivery, `
    import { createInterface } from 'node:readline';
    process.stdout.write(${JSON.stringify(`${JSON.stringify(hook)}\n`)});
    for await (const line of createInterface({input:process.stdin})) {
      const reply = JSON.parse(line);
      if (reply.response.request_id !== 'native-1') process.exitCode = 2;
      process.stdout.write(JSON.stringify({type:'peer_received'})+'\\n');
    }
  `);
  const exit = once(process, "exit");
  const lines = createInterface({ input: process.stdout })[Symbol.asyncIterator]();
  assert.deepEqual(JSON.parse((await lines.next()).value), hook);
  const id = delivery.claim(structuredClone(hook.request.input), "tool-1");
  assert.equal(id, "native-1");
  const observed = [];
  delivery.expect(id, (success) => observed.push(success));
  assert.deepEqual(observed, []);
  const encoded = `${JSON.stringify(response)}\n`;
  // Neither a partial frame nor an SDK queue write can complete the request.
  await new Promise((resolve, reject) => process.stdin.write(encoded.slice(0, -1), (e) => e ? reject(e) : resolve()));
  assert.deepEqual(observed, []);
  await new Promise((resolve, reject) => process.stdin.write("\n", (e) => e ? reject(e) : resolve()));
  assert.deepEqual(observed, [true]);
  assert.equal(JSON.parse((await lines.next()).value).type, "peer_received");
  process.stdin.end();
  assert.deepEqual(await exit, [0, null]);
});

test("native exit fails pending delivery and rejects stale or ambiguous hook identity", async () => {
  const delivery = new NativeDelivery();
  const process = child(delivery, "process.exit(0)");
  const observed = [];
  delivery.expect("pending", (success) => observed.push(success));
  await once(process, "exit");
  assert.deepEqual(observed, [false]);
  assert.throws(() => delivery.expect("later", () => {}));
  assert.throws(() => delivery.claim(hook.request.input, "tool-1"));

  const other = new NativeDelivery();
  other.observe(hook);
  other.observe({ ...hook, request_id: "native-2" });
  assert.throws(() => other.claim(hook.request.input, "tool-1"));
  other.close();
});

test("installed SDK hook callback is correlated to the native stdin response", async () => {
  const { query } = await import("@anthropic-ai/claude-agent-sdk");
  const delivery = new NativeDelivery();
  let finish;
  const received = new Promise((resolve) => { finish = resolve; });
  let native;
  const stream = query({
    prompt: (async function* () { yield { type: "user", message: { role: "user", content: "fixture" } }; })(),
    options: {
      pathToClaudeCodeExecutable: "/fixture/claude", tools: [], settingSources: [],
      hooks: { PermissionRequest: [{ hooks: [async (input, toolUseId) => {
        const id = delivery.claim(input, toolUseId);
        delivery.expect(id, finish);
        return { hookSpecificOutput: { hookEventName: "PermissionRequest", decision: { behavior: "deny" } } };
      }] }] },
      spawnClaudeCodeProcess: () => {
        native = child(delivery, `
          import { createInterface } from 'node:readline';
          let initialized = false;
          for await (const line of createInterface({input:process.stdin})) {
            const m = JSON.parse(line);
            if (m.type === 'control_request' && m.request.subtype === 'initialize') {
              process.stdout.write(JSON.stringify({type:'control_response', response:{subtype:'success', request_id:m.request_id, response:{}}})+'\\n');
              const callback = m.request.hooks.PermissionRequest[0].hookCallbackIds[0];
              const hook = ${JSON.stringify(hook)};
              hook.request.callback_id = callback;
              process.stdout.write(JSON.stringify(hook)+'\\n');
              initialized = true;
            } else if (m.type === 'control_response') {
              if (!initialized || m.response.response.hookSpecificOutput.decision.behavior !== 'deny') process.exitCode = 2;
            }
          }
        `);
        return native;
      },
    },
  });
  try {
    await stream.initializationResult();
    assert.equal(await received, true);
  } finally {
    const exit = native && once(native, "exit");
    stream.close();
    if (exit) assert.deepEqual(await exit, [0, null]);
  }
});
