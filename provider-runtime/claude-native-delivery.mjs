import { spawn } from "node:child_process";
import { Transform, Writable } from "node:stream";
import { StringDecoder } from "node:string_decoder";
import { isDeepStrictEqual } from "node:util";

// Observe the SDK's public process transport without becoming its protocol dispatcher.
// The SDK owns hook dispatch, cancellation, response serialization and graceful close.
export class NativeDelivery {
  #hooks = new Map();
  #pending = new Map();
  #closed = false;

  observe(message) {
    if (message.type !== "control_request" || message.request?.subtype !== "hook_callback") return;
    const { request_id: id, request } = message;
    if (typeof id !== "string" || !id || this.#closed || this.#hooks.has(id) || this.#hooks.size >= 128) {
      throw new Error("invalid native hook custody");
    }
    this.#hooks.set(id, request);
  }

  claim(input, toolUseId) {
    const matches = [...this.#hooks].filter(([, request]) =>
      request.tool_use_id === toolUseId && isDeepStrictEqual(request.input, input));
    if (matches.length !== 1) throw new Error("ambiguous native hook custody");
    const [id] = matches[0];
    this.#hooks.delete(id);
    return id;
  }

  expect(id, delivered) {
    if (typeof id !== "string" || !id || this.#closed || this.#pending.has(id) || this.#pending.size >= 128) {
      throw new Error("invalid native response custody");
    }
    this.#pending.set(id, delivered);
  }

  written(message) {
    if (message.type !== "control_response") return;
    const id = message.response?.request_id;
    const delivered = this.#pending.get(id);
    this.#pending.delete(id);
    // Unclaimed callbacks are rejected by the SDK, but cannot retain stale custody.
    this.#hooks.delete(id);
    if (delivered) delivered(message.response.subtype === "success");
  }

  close() {
    this.#closed = true;
    this.#hooks.clear();
    const pending = [...this.#pending.values()];
    this.#pending.clear();
    for (const delivered of pending) delivered(false);
  }

  spawn = (options) => {
    const child = spawn(options.command, options.args, {
      cwd: options.cwd,
      env: options.env,
      signal: options.signal,
      stdio: ["pipe", "pipe", "ignore"],
      windowsHide: true,
    });
    const nativeInput = child.stdin;
    const nativeOutput = child.stdout;
    const incoming = frames((message) => this.observe(message));
    const outgoing = frames((message) => this.written(message));
    const output = new Transform({
      transform(chunk, _encoding, callback) {
        try {
          incoming(chunk);
          callback(null, chunk);
        } catch (error) { callback(error); }
      },
    });
    const input = new Writable({
      write(chunk, _encoding, callback) {
        nativeInput.write(chunk, (error) => {
          if (error) return callback(error);
          try {
            // Native stdin's write callback, not the SDK's synchronous queue return.
            outgoing(chunk);
            callback();
          } catch (failure) { callback(failure); }
        });
      },
      final(callback) { nativeInput.end(callback); },
      destroy(error, callback) {
        nativeInput.destroy();
        callback(error);
      },
    });
    nativeInput.on("error", (error) => input.destroy(error));
    nativeOutput.on("error", (error) => output.destroy(error));
    input.on("error", () => this.close());
    output.on("error", () => this.close());
    child.once("exit", () => this.close());
    child.once("error", () => this.close());
    nativeOutput.pipe(output);
    child.stdin = input;
    child.stdout = output;
    return child;
  };
}

function frames(accept) {
  const decoder = new StringDecoder("utf8");
  let pending = "";
  return (chunk) => {
    pending += decoder.write(chunk);
    let newline;
    while ((newline = pending.indexOf("\n")) !== -1) {
      const line = pending.slice(0, newline);
      pending = pending.slice(newline + 1);
      if (line.trim()) accept(JSON.parse(line));
    }
  };
}
