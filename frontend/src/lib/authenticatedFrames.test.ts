import { describe, expect, it } from "vitest";
import {
  decodeAuthenticatedFrame,
  deriveAuthenticatedFrameKey,
  encodeAuthenticatedFrame,
} from "./authenticatedFrames";

const PROOF_KEY = "b".repeat(64);
const CONNECTION_NONCE = "c".repeat(64);

async function channelKey() {
  return deriveAuthenticatedFrameKey(PROOF_KEY, CONNECTION_NONCE);
}

describe("authenticated WebSocket frames", () => {
  it("round-trips exact payload bytes for each direction", async () => {
    const key = await channelKey();
    const payload = '{"op":"ping","nonce":"정확한 bytes"}';
    const envelope = await encodeAuthenticatedFrame(
      key,
      CONNECTION_NONCE,
      "client",
      1,
      payload
    );
    await expect(decodeAuthenticatedFrame(
      key,
      CONNECTION_NONCE,
      "client",
      1,
      envelope
    )).resolves.toBe(payload);
  });

  it("rejects catch-up content mutation under a genuine envelope proof", async () => {
    const key = await channelKey();
    const original = JSON.stringify({
      op: "event",
      stream: "room_events",
      latest_seq: 2,
      events: [{ id: "evt-2", room_id: "general", seq: 2, type: "message_final", content: "real" }],
    });
    const envelope = JSON.parse(await encodeAuthenticatedFrame(
      key,
      CONNECTION_NONCE,
      "server",
      1,
      original
    )) as Record<string, unknown>;
    envelope.payload = btoa(JSON.stringify({
      op: "event",
      stream: "room_events",
      latest_seq: 2,
      events: [{ id: "evt-2", room_id: "general", seq: 2, type: "message_final", content: "forged" }],
    }));
    await expect(decodeAuthenticatedFrame(
      key,
      CONNECTION_NONCE,
      "server",
      1,
      JSON.stringify(envelope)
    )).rejects.toThrow("proof is invalid");
  });

  it("rejects command mutation, replay, counter gaps, and reflection", async () => {
    const key = await channelKey();
    const command = JSON.stringify({
      op: "command",
      request_id: "same-id",
      action: "message.send",
      payload: {
        content: "real",
        attachment_ids: [`ma_${"a".repeat(32)}`],
      },
    });
    const envelope = await encodeAuthenticatedFrame(
      key,
      CONNECTION_NONCE,
      "client",
      1,
      command
    );
    const mutated = JSON.parse(envelope) as Record<string, unknown>;
    mutated.payload = btoa(JSON.stringify({
      op: "command",
      request_id: "same-id",
      action: "message.send",
      payload: {
        content: "real",
        attachment_ids: [`ma_${"b".repeat(32)}`],
      },
    }));
    await expect(decodeAuthenticatedFrame(
      key,
      CONNECTION_NONCE,
      "client",
      1,
      JSON.stringify(mutated)
    )).rejects.toThrow("proof is invalid");
    await expect(decodeAuthenticatedFrame(
      key,
      CONNECTION_NONCE,
      "client",
      2,
      envelope
    )).rejects.toThrow("counter is invalid");
    await expect(decodeAuthenticatedFrame(
      key,
      CONNECTION_NONCE,
      "server",
      1,
      envelope
    )).rejects.toThrow("proof is invalid");
  });
});
