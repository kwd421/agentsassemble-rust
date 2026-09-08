import assert from "node:assert/strict";
import { once } from "node:events";
import { createInterface } from "node:readline";
import test from "node:test";
import { NativeDelivery } from "./claude-native-delivery.mjs";
import { OwnerRequests } from "./claude-owner-requests.mjs";

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

test("installed SDK questions, plan approval and permissions wait for owner delivery receipts", async () => {
  const { query } = await import("@anthropic-ai/claude-agent-sdk");
  for (const name of ["AskUserQuestion", "ExitPlanMode", "Bash"]) {
    const delivery = new NativeDelivery();
    let offer, delivered;
    const opened = new Promise((resolve) => { offer = resolve; });
    const written = new Promise((resolve) => { delivered = resolve; });
    const failures = [];
    const requests = new OwnerRequests({ id: "session", active: { turnId: "turn-1" } }, delivery, async (message) => {
      if (message.type === "provider_request") offer(message);
      if (message.type === "request_delivered") delivered(message);
    }, (error) => failures.push(error));
    const toolInput = name === "AskUserQuestion" ? { questions: [{ question: "Format?", header: "Format", options: [{label:"Summary"}], multiSelect:false }] } : { private_input: "not persisted" };
    const nativeRequest = { type: "control_request", request_id: "native-1", request: name === "Bash"
      ? { subtype: "can_use_tool", tool_name: name, input: toolInput, tool_use_id: "tool-1" }
      : { subtype: "hook_callback", tool_use_id: "tool-1", input: { session_id: "session", hook_event_name: "PreToolUse", tool_name: name, tool_input: toolInput } } };
    let native;
    const stream = query({
      prompt: (async function* () { yield { type: "user", message: { role: "user", content: "fixture" } }; })(),
      options: {
        pathToClaudeCodeExecutable: "/fixture/claude", tools: [], settingSources: [],
        hooks: requests.hooks, canUseTool: requests.canUseTool,
        spawnClaudeCodeProcess: () => {
          native = child(delivery, `
            import { createInterface } from 'node:readline';
            for await (const line of createInterface({input:process.stdin})) {
              const m = JSON.parse(line);
              if (m.type === 'control_request' && m.request.subtype === 'initialize') {
                process.stdout.write(JSON.stringify({type:'control_response', response:{subtype:'success', request_id:m.request_id, response:{}}})+'\\n');
                const request = ${JSON.stringify(nativeRequest)};
                if (request.request.subtype === 'hook_callback') request.request.callback_id = m.request.hooks.PreToolUse[0].hookCallbackIds[0];
                process.stdout.write(JSON.stringify(request)+'\\n');
              } else if (m.type === 'control_response') {
                const output = m.response.response;
                const decision = output.hookSpecificOutput ?? output;
                if ((decision.permissionDecision ?? decision.behavior) !== 'allow') process.exitCode = 2;
                if (${JSON.stringify(name)} === 'AskUserQuestion' && decision.updatedInput.answers['Format?'] !== 'Summary') process.exitCode = 2;
                if (${JSON.stringify(name)} !== 'AskUserQuestion' && decision.updatedInput.private_input !== 'not persisted') process.exitCode = 2;
              }
            }
          `);
          return native;
        },
      },
    });
    try {
      await stream.initializationResult();
      const request = await opened;
      assert.equal(request.turn_id, "turn-1");
      assert.equal(JSON.stringify(request).includes("not persisted"), false);
      const id = request.request.provider_request_id;
      assert.throws(() => requests.accept({type:"request_complete", request_id:id}));
      requests.accept({type:"request_answer", request_id:id, resolution: name === "AskUserQuestion"
        ? {response_kind:"answers", answers:{"question-0":["Summary"]}}
        : {response_kind:"option", option_id:"allow-once"}});
      assert.equal((await written).delivered, true);
      const finished = requests.finishTurn();
      assert.equal(await Promise.race([finished.then(() => false), new Promise((resolve) => setImmediate(() => resolve(true)))]), true);
      requests.accept({type:"request_complete", request_id:id});
      await finished;
      assert.deepEqual(failures, []);
    } finally {
      requests.close();
      const exit = native && once(native, "exit");
      stream.close();
      if (exit) assert.deepEqual(await exit, [0, null]);
    }
  }
});

test("native cancellation rejects a racing owner answer", async () => {
  for (const answeredFirst of [false, true]) {
    const events = [];
    const failures = [];
    const delivery = new NativeDelivery();
    const requests = new OwnerRequests({id:"session", active:{turnId:"turn-1"}}, delivery, async (event) => events.push(event), (error) => failures.push(error));
    const cancelled = new AbortController();
    const answer = requests.canUseTool("Bash", {}, {requestId:"native-1", signal:cancelled.signal});
    const id = events[0].request.provider_request_id;
    const resolve = () => requests.accept({type:"request_answer", request_id:id, resolution:{response_kind:"option", option_id:"allow-once"}});
    if (answeredFirst) resolve();
    cancelled.abort();
    if (!answeredFirst) resolve();
    assert.equal((await answer).behavior, "deny");
    assert.equal(events[1].type, "request_cancelled");
    requests.accept({type:"request_complete", request_id:id});
    await requests.finishTurn();
    delivery.written(response);
    assert.equal(events.length, 2);
    assert.deepEqual(failures, []);
    requests.close();
  }
});
