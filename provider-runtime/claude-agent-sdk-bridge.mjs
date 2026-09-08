import { randomUUID } from "node:crypto";
import { pathToFileURL } from "node:url";
import { NativeDelivery } from "./claude-native-delivery.mjs";
import { OwnerRequests } from "./claude-owner-requests.mjs";

const MAX_INPUT_LINE_BYTES = 256 * 1024;
const MAX_OUTPUT_LINE_BYTES = 256 * 1024;
const MAX_RESULT_BYTES = 128 * 1024;
const MODEL_ID = /^claude-(?:fable|haiku|opus|sonnet)-\d+(?:-\d+)?$/;
const SESSION_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;
const EFFORTS = new Set(["low", "medium", "high", "xhigh", "max"]);

class InputQueue {
  #values = [];
  #waiters = [];
  #closed = false;

  push(value) {
    if (this.#closed) throw new Error("input queue closed");
    const waiter = this.#waiters.shift();
    if (waiter) waiter({ value, done: false });
    else this.#values.push(value);
  }

  close() {
    this.#closed = true;
    for (const waiter of this.#waiters.splice(0)) waiter({ done: true });
  }

  [Symbol.asyncIterator]() {
    return this;
  }

  next() {
    const value = this.#values.shift();
    if (value !== undefined) return Promise.resolve({ value, done: false });
    if (this.#closed) return Promise.resolve({ done: true });
    return new Promise((resolve) => this.#waiters.push(resolve));
  }
}

let writes = Promise.resolve();
let fatalEmitted = false;

function emit(payload) {
  const encoded = `${JSON.stringify(payload)}\n`;
  if (Buffer.byteLength(encoded) > MAX_OUTPUT_LINE_BYTES) {
    throw new Error("output exceeded protocol limit");
  }
  writes = writes.then(
    () =>
      new Promise((resolve, reject) => {
        process.stdout.write(encoded, (error) => (error ? reject(error) : resolve()));
      }),
  );
  return writes;
}

async function emitFatal(code) {
  if (fatalEmitted) return;
  fatalEmitted = true;
  await emit({ type: "fatal", code });
}

async function* inputLines() {
  let pending = Buffer.alloc(0);
  for await (const chunk of process.stdin) {
    const bytes = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk);
    pending = Buffer.concat([pending, bytes]);
    let newline;
    while ((newline = pending.indexOf(10)) !== -1) {
      if (newline > MAX_INPUT_LINE_BYTES) throw new Error("input exceeded protocol limit");
      const line = pending.subarray(0, newline);
      pending = pending.subarray(newline + 1);
      yield line.at(-1) === 13 ? line.subarray(0, -1).toString("utf8") : line.toString("utf8");
    }
    if (pending.length > MAX_INPUT_LINE_BYTES) throw new Error("input exceeded protocol limit");
  }
  if (pending.length) yield pending.toString("utf8");
}

function parseLine(line) {
  if (!line) throw new Error("empty protocol line");
  const value = JSON.parse(line);
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("invalid protocol value");
  }
  return value;
}

function exactModels(models) {
  const exact = new Map();
  for (const model of Array.isArray(models) ? models : []) {
    const id = MODEL_ID.test(model?.resolvedModel ?? "")
      ? model.resolvedModel
      : MODEL_ID.test(model?.value ?? "")
        ? model.value
        : "";
    if (!id || exact.has(id)) continue;
    const efforts = Array.isArray(model.supportedEffortLevels)
      ? model.supportedEffortLevels.filter((effort) => EFFORTS.has(effort))
      : [];
    exact.set(id, {
      id,
      label: typeof model.displayName === "string" && model.displayName.trim() ? model.displayName : id,
      efforts: [...new Set(efforts)],
      fast: model.supportsFastMode === true,
    });
  }
  return [...exact.values()].sort((left, right) => left.id.localeCompare(right.id));
}

async function loadSdk(path) {
  if (!path) throw new Error("missing SDK module");
  const sdk = await import(pathToFileURL(path).href);
  if (typeof sdk.query !== "function") throw new Error("invalid SDK module");
  return sdk;
}

function baseOptions(claudePath) {
  if (!claudePath) throw new Error("missing Claude executable");
  return {
    pathToClaudeCodeExecutable: claudePath,
    settingSources: [],
    strictMcpConfig: true,
    tools: [],
    permissionMode: "dontAsk",
    skills: [],
    plugins: [],
    promptSuggestions: false,
    agentProgressSummaries: false,
  };
}

async function catalog(sdk, claudePath) {
  const input = new InputQueue();
  const query = sdk.query({ prompt: input, options: baseOptions(claudePath) });
  try {
    await emit({ type: "catalog", models: exactModels(await query.supportedModels()) });
  } finally {
    input.close();
    query.close();
  }
}

function requireString(value, name) {
  if (typeof value !== "string" || !value.trim() || value !== value.trim()) {
    throw new Error(`invalid ${name}`);
  }
  return value;
}

function sessionOptions(command, claudePath, sessionId) {
  const workspace = requireString(command.workspace, "workspace");
  const model = requireString(command.model, "model");
  const effort = requireString(command.reasoning_effort, "reasoning effort");
  const permission = command.permission_mode;
  const tier = command.service_tier;
  if (!MODEL_ID.test(model) || !EFFORTS.has(effort)) throw new Error("invalid runtime selection");
  if (!new Set(["meeting_read_only", "workspace_write"]).has(permission)) {
    throw new Error("invalid permission mode");
  }
  if (!new Set(["default", "fast"]).has(tier)) throw new Error("invalid service tier");
  const portal = command.room_portal;
  if (!portal || typeof portal !== "object") throw new Error("missing room portal");
  const url = requireString(portal.url, "room portal URL");
  const bearer = requireString(portal.bearer_token, "room portal bearer");
  const options = {
    ...baseOptions(claudePath),
    cwd: workspace,
    model,
    effort,
    permissionMode: permission === "workspace_write" ? "acceptEdits" : "dontAsk",
    tools: permission === "workspace_write" ? { type: "preset", preset: "claude_code" } : ["AskUserQuestion"],
    allowedTools: ["mcp__agentsassemble_room__*"],
    mcpServers: {
      agentsassemble_room: {
        type: "http",
        url,
        headers: { Authorization: `Bearer ${bearer}` },
        alwaysLoad: true,
      },
    },
    systemPrompt: { type: "preset", preset: "claude_code", snapshot: true },
    ...(tier === "fast" ? { settings: { fastMode: true, fastModePerSessionOptIn: true } } : {}),
    ...(command.resume_session_id ? { resume: sessionId } : { sessionId }),
  };
  return { options, workspace, model, effort, tier, permission };
}

function validateInitialization(models, model, effort, tier) {
  const selected = exactModels(models).find((candidate) => candidate.id === model);
  if (!selected || !selected.efforts.includes(effort) || (tier === "fast" && !selected.fast)) {
    throw new Error("runtime selection not supported");
  }
}

function validInit(message, active, session) {
  return (
    message.session_id === session.id &&
    message.cwd === session.workspace &&
    message.model === session.model &&
    message.effort === session.effort &&
    (session.tier === "fast" ? message.fast_mode_state === "on" : message.fast_mode_state !== "on") &&
    message.permissionMode === (session.permission === "workspace_write" ? "acceptEdits" : "dontAsk") &&
    Array.isArray(message.tools) &&
    (session.permission !== "meeting_read_only" ||
      message.tools.every((tool) => tool === "AskUserQuestion" || tool.startsWith("mcp__agentsassemble_room__"))) &&
    Array.isArray(message.mcp_servers) &&
    message.mcp_servers.length === 1 &&
    message.mcp_servers.every(
      (server) => server?.name === "agentsassemble_room" && server.status === "connected",
    ) &&
    active !== null
  );
}

function validResult(message, active, session) {
  return (
    active !== null &&
    active.initialized &&
    message.session_id === session.id &&
    message.user_message_uuid === active.sdkTurnId &&
    typeof message.uuid === "string" &&
    message.uuid.length > 0 &&
    message.subtype === "success" &&
    message.is_error === false &&
    message.terminal_reason === "completed" &&
    message.modelUsage &&
    Object.hasOwn(message.modelUsage, session.model) &&
    (session.tier === "fast" ? message.fast_mode_state === "on" : message.fast_mode_state !== "on") &&
    (message.queued_turn_count ?? 0) === 0 &&
    typeof message.result === "string" &&
    Buffer.byteLength(message.result) <= MAX_RESULT_BYTES
  );
}

async function session(sdk, claudePath, command, commands) {
  const resume = command.resume_session_id ?? "";
  const id = resume ? requireString(resume, "resume session") : randomUUID();
  if (!SESSION_ID.test(id)) throw new Error("invalid session ID");
  const queue = new InputQueue();
  const configured = sessionOptions(command, claudePath, id);
  const delivery = new NativeDelivery();
  configured.options.spawnClaudeCodeProcess = delivery.spawn;
  const state = { id, ...configured, shuttingDown: false, active: null, failed: false };
  const fail = () => {
    state.failed = true;
    emitFatal("claude_sdk_request_failed").catch(() => {});
    process.stdin.destroy();
  };
  const requests = new OwnerRequests(state, delivery, emit, fail);
  configured.options.hooks = requests.hooks;
  configured.options.canUseTool = requests.canUseTool;
  const query = sdk.query({ prompt: queue, options: configured.options });
  const initialization = await query.initializationResult();
  validateInitialization(initialization.models, configured.model, configured.effort, configured.tier);
  await emit({ type: "ready", session_id: id, reused: Boolean(resume), model: configured.model });

  const reader = (async () => {
    for await (const message of query) {
      if (message?.type === "system" && message.subtype === "init") {
        if (!validInit(message, state.active, state)) throw new Error("invalid SDK initialization");
        state.active.initialized = true;
      } else if (message?.type === "result") {
        const active = state.active;
        if (!validResult(message, active, state)) throw new Error("invalid SDK result");
        active.closing = true;
        await requests.finishTurn();
        if (state.failed || state.shuttingDown) throw new Error("request custody ended");
        await emit({
          type: "turn_result",
          turn_id: active.turnId,
          provider_turn_id: message.uuid,
          session_id: state.id,
          content: message.result,
        });
        state.active = null;
      }
    }
    if (!state.shuttingDown) throw new Error("SDK stream ended");
  })().catch(async () => {
    state.failed = true;
    await emitFatal("claude_sdk_protocol_failed").catch(() => {});
    process.stdin.destroy();
  });

  try {
    for await (const line of commands) {
      const next = parseLine(line);
      if (next.type === "turn") {
        if (state.active || state.failed) throw new Error("turn already active");
        const turnId = requireString(next.turn_id, "turn ID");
        const input = requireString(next.input, "turn input");
        const sdkTurnId = randomUUID();
        state.active = { turnId, sdkTurnId, initialized: false };
        queue.push({
          type: "user",
          message: { role: "user", content: input },
          parent_tool_use_id: null,
          uuid: sdkTurnId,
          session_id: state.id,
        });
      } else if (requests.accept(next)) {
        continue;
      } else if (next.type === "shutdown") {
        state.shuttingDown = true;
        requests.close();
        queue.close();
        query.close();
        await emit({ type: "stopped" });
        break;
      } else {
        throw new Error("unsupported protocol command");
      }
    }
  } finally {
    state.shuttingDown = true;
    requests.close();
    delivery.close();
    queue.close();
    query.close();
    await reader;
  }
  if (state.failed) throw new Error("SDK reader failed");
}

async function main() {
  const [sdkPath, claudePath, mode] = process.argv.slice(2);
  const sdk = await loadSdk(sdkPath);
  if (mode === "catalog") {
    await catalog(sdk, claudePath);
    return;
  }
  if (mode !== "session") throw new Error("invalid bridge mode");
  const commands = inputLines();
  const first = await commands.next();
  if (first.done) throw new Error("missing initialize command");
  const command = parseLine(first.value);
  if (command.type !== "initialize") throw new Error("expected initialize command");
  await session(sdk, claudePath, command, commands);
}

main().catch(async () => {
  process.stdin.destroy();
  await emitFatal("claude_sdk_bridge_failed").catch(() => {});
  process.exitCode = 1;
});
