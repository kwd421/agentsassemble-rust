// Test-only SDK decorator. The runner substitutes the installed SDK's absolute URL.
import { query as nativeQuery } from "__NATIVE_SDK_URL__";
import { appendFileSync } from "node:fs";
const record = (event) => appendFileSync("__EVIDENCE_PATH__", `${JSON.stringify(event)}\n`, { mode: 0o600 });
import { randomUUID } from "node:crypto";

export function query(args) {
  const native = nativeQuery(args);
  if (args.options.persistSession === false) return native;
  return new Proxy(native, {
    get(target, key) {
      if (key !== Symbol.asyncIterator) {
        const value = Reflect.get(target, key, target);
        return typeof value === "function" ? value.bind(target) : value;
      }
      return async function* () {
        const stream = target[Symbol.asyncIterator]();
        let compacted = false;
        for (;;) {
          const next = await stream.next();
          if (next.done) return;
          const message = next.value;
          const content = message.message?.content;
          const calls = (Array.isArray(content) ? content : []).filter((part) => part.type === "tool_use").map((part) => part.name);
          record({ type: message.type, subtype: message.subtype, calls,
            model: message.message?.model,
            usage: Object.fromEntries(["input_tokens", "output_tokens", "cache_creation_input_tokens", "cache_read_input_tokens"]
              .filter((key) => typeof message.message?.usage?.[key] === "number")
              .map((key) => [key, message.message.usage[key]])) });
          if (!compacted && message.type === "result" && message.subtype === "success") {
            compacted = true;
            args.prompt.push({ type: "user", message: { role: "user", content: "/compact" },
              parent_tool_use_id: null, uuid: randomUUID(), session_id: message.session_id });
            let boundary = false;
            record({ compact_submitted: true });
            for (;;) {
              const compact = await stream.next();
              if (compact.done) throw new Error("compact stream ended");
              const event = compact.value;
              if (event.session_id && event.session_id !== message.session_id) throw new Error("compact session changed");
              if (event.type === "system" && event.subtype === "compact_boundary") {
                boundary = true;
                record({ compact_boundary: true, trigger: event.compact_metadata.trigger, pre_tokens: event.compact_metadata.pre_tokens, post_tokens: event.compact_metadata.post_tokens, same_session: true });
              }
              if (event.type === "result") {
                record({ compact_result: event.subtype, boundary, same_session: true });
                if (!boundary || event.is_error || event.subtype !== "success") throw new Error("compact failed");
                break;
              }
            }
          }
          // Keep all room-turn validation in the production bridge. Only the
          // slash-command exchange is consumed here; the original result is intact.
          yield message;
        }
      };
    },
  });
}
