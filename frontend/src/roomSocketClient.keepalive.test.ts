import { afterEach, describe, expect, it, vi } from "vitest";
import { RoomSocketSayError } from "./roomSocketClient";
import {
  flushPromises,
  handshakeFrames,
  openHarness,
  receiveServerFrame,
  sentClientFrame,
} from "./test/roomSocketHarness";

const QUIET_KEEPALIVE_MS = 3 * 60_000;

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("canonical room socket quiet keepalive", () => {
  it("sends one ping after client silence and stops with the connection", async () => {
    vi.useFakeTimers();
    const { handle, opened, sockets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await opened;
    expect(handle.ready()).toBe(true);

    await vi.advanceTimersByTimeAsync(QUIET_KEEPALIVE_MS - 1);
    expect(sockets[0].sent).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(1);
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const firstPing = sentClientFrame(sockets[0]);
    expect(firstPing).toEqual({ op: "ping", nonce: "keepalive-1" });

    receiveServerFrame(sockets[0], {
      op: "pong",
      nonce: firstPing.nonce,
    });
    await flushPromises();
    await vi.advanceTimersByTimeAsync(QUIET_KEEPALIVE_MS);
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(3));
    expect(sentClientFrame(sockets[0], 2)).toEqual({
      op: "ping",
      nonce: "keepalive-2",
    });

    handle.close();
    await vi.advanceTimersByTimeAsync(QUIET_KEEPALIVE_MS * 2);
    expect(sockets[0].sent).toHaveLength(3);
  });

  it("measures silence from the most recent command", async () => {
    vi.useFakeTimers();
    const { handle, opened, sockets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await opened;
    expect(handle.ready()).toBe(true);

    await vi.advanceTimersByTimeAsync(QUIET_KEEPALIVE_MS - 10_000);
    const commandIssuedAt = Date.now();
    const pending = handle.command("message.send", { content: "still here" });
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const command = sentClientFrame(sockets[0]);
    receiveServerFrame(sockets[0], {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: command.request_id,
      action: "message.send",
      result: {
        event: {
          v: 1,
          id: "evt-1",
          room_id: "general",
          seq: 1,
          type: "message_final",
          content: "still here",
        },
        event_seq: 1,
      },
    });
    await expect(pending).resolves.toMatchObject({ accepted: true });

    const elapsedSinceCommand = Date.now() - commandIssuedAt;
    await vi.advanceTimersByTimeAsync(QUIET_KEEPALIVE_MS - elapsedSinceCommand - 1);
    expect(sockets[0].sent).toHaveLength(2);
    await vi.advanceTimersByTimeAsync(1_000);
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(3));
    expect(sentClientFrame(sockets[0], 2)).toMatchObject({
      op: "ping",
    });
    handle.close();
  });

  it("fails the exact connection on a mismatched pong", async () => {
    vi.useFakeTimers();
    const errors: RoomSocketSayError[] = [];
    const { handle, opened, sockets } = openHarness({
      onError: (error) => {
        if (error instanceof RoomSocketSayError) errors.push(error);
      },
    });
    await flushPromises();
    sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await opened;
    expect(handle.ready()).toBe(true);
    await vi.advanceTimersByTimeAsync(QUIET_KEEPALIVE_MS);
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));

    receiveServerFrame(sockets[0], {
      op: "pong",
      nonce: "not-the-owned-keepalive",
    });
    await vi.waitFor(() =>
      expect(errors.at(-1)?.category).toBe("keepalive_response_invalid")
    );
    expect(sockets[0].readyState).toBe(WebSocket.CLOSED);
    handle.close();
  });
});
