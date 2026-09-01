import { afterEach, describe, expect, it, vi } from "vitest";
import {
  event,
  flushPromises,
  handshakeFrames,
  openHarness,
  receiveServerFrame,
  sentClientFrame,
} from "./test/roomSocketHarness";
import { scheduleUncertainCommandRetry } from "./roomSocketRetryPolicy";

const COMMAND_TIMEOUT_MS = 20_000;
const UNRESOLVED_DELAYS_MS = [500, 1_000, 2_000, 4_000, 8_000, 16_000, 30_000];

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("room socket exact-command retry", () => {
  it("counts one uncertain outcome at most once per connection generation", () => {
    const retry = { retryAttempt: 0, retryCountedGeneration: 0, retryNotBefore: 0 };
    expect(scheduleUncertainCommandRetry(retry, 7)).toBe("retry");
    expect(scheduleUncertainCommandRetry(retry, 7)).toBe("already_counted");
    expect(retry.retryAttempt).toBe(1);
  });

  it("settles a received terminal ACK before charging its closing connection", async () => {
    vi.useFakeTimers();
    const { handle, sockets } = openHarness();
    await flushPromises();
    await openReadyConnection(0, handle, sockets);
    const pendingCommand = handle.command("message.send", { content: "received before close" });
    void pendingCommand.catch(() => {});

    for (let attempt = 0; attempt < 7; attempt += 1) {
      await vi.waitFor(() => expect(sockets[attempt].sent).toHaveLength(2));
      const command = sentClientFrame(sockets[attempt]);
      receiveServerFrame(sockets[attempt], unresolved(command));
      await vi.waitFor(() => expect(sockets[attempt].readyState).toBe(WebSocket.CLOSED));
      await vi.advanceTimersByTimeAsync(500);
      await openReadyConnection(attempt + 1, handle, sockets);
      await vi.advanceTimersByTimeAsync(Math.max(0, UNRESOLVED_DELAYS_MS[attempt] - 500));
    }

    await vi.waitFor(() => expect(sockets[7].sent).toHaveLength(2));
    const finalCommand = sentClientFrame(sockets[7]);
    receiveServerFrame(sockets[7], {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: finalCommand.request_id,
      action: "message.send",
      result: { event: event(1), event_seq: 1 },
      deduplicated: true,
    });
    sockets[7].close();
    await flushPromises();

    await expect(pendingCommand).resolves.toMatchObject({ resolution: "committed" });
    handle.close();
  });

  it("charges an abrupt post-send close to the same exact-command budget", async () => {
    vi.useFakeTimers();
    const { handle, sockets } = openHarness();
    await flushPromises();
    await openReadyConnection(0, handle, sockets);
    const pendingCommand = handle.command("message.send", { content: "closed transport" });
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const firstCommand = sentClientFrame(sockets[0]);

    sockets[0].close();
    await vi.advanceTimersByTimeAsync(500);
    await openReadyConnection(1, handle, sockets);
    await vi.waitFor(() => expect(sockets[1].sent).toHaveLength(2));
    const replayed = sentClientFrame(sockets[1]);
    expect(replayed).toEqual(firstCommand);
    receiveServerFrame(sockets[1], {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: replayed.request_id,
      action: "message.send",
      result: { event: event(1), event_seq: 1 },
      deduplicated: true,
    });
    await expect(pendingCommand).resolves.toMatchObject({ resolution: "committed" });
    handle.close();
  });

  it("replays exact serialized bytes after ACK silence and unresolved replies", async () => {
    vi.useFakeTimers();
    const { handle, sockets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const firstFrames = handshakeFrames(0, 0);
    sockets[0].receive(firstFrames.receipt);
    sockets[0].receiveRaw(firstFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));

    const commandPayload = { content: "commit once" };
    const pendingCommand = handle.command("message.send", commandPayload);
    let settled = false;
    void pendingCommand.then(
      () => { settled = true; },
      () => { settled = true; }
    );
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const firstCommand = sentClientFrame(sockets[0]);
    commandPayload.content = "mutated after send";

    await vi.advanceTimersByTimeAsync(COMMAND_TIMEOUT_MS);
    expect(sockets[0].readyState).toBe(WebSocket.CLOSED);
    expect(settled).toBe(false);

    await vi.advanceTimersByTimeAsync(500);
    sockets[1].open();
    const secondFrames = handshakeFrames(0, 0);
    sockets[1].receive(secondFrames.receipt);
    sockets[1].receiveRaw(secondFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    await vi.waitFor(() => expect(sockets[1].sent).toHaveLength(2));
    const replayedCommand = sentClientFrame(sockets[1]);
    expect(replayedCommand).toEqual(firstCommand);

    receiveServerFrame(sockets[1], unresolved(replayedCommand));
    await vi.waitFor(() => expect(sockets[1].readyState).toBe(WebSocket.CLOSED));
    expect(settled).toBe(false);

    await vi.advanceTimersByTimeAsync(500);
    sockets[2].open();
    const thirdFrames = handshakeFrames(0, 0);
    sockets[2].receive(thirdFrames.receipt);
    sockets[2].receiveRaw(thirdFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    await vi.waitFor(() => expect(sockets[2].sent).toHaveLength(2));
    const replayedAfterUnresolved = sentClientFrame(sockets[2]);
    expect(replayedAfterUnresolved).toEqual(firstCommand);

    receiveServerFrame(sockets[2], unresolved(replayedAfterUnresolved));
    await vi.waitFor(() => expect(sockets[2].readyState).toBe(WebSocket.CLOSED));
    await vi.advanceTimersByTimeAsync(500);
    sockets[3].open();
    const fourthFrames = handshakeFrames(0, 0);
    sockets[3].receive(fourthFrames.receipt);
    sockets[3].receiveRaw(fourthFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    expect(sockets[3].sent).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(500);
    await vi.waitFor(() => expect(sockets[3].sent).toHaveLength(2));
    const backedOffReplay = sentClientFrame(sockets[3]);
    expect(backedOffReplay).toEqual(firstCommand);

    receiveServerFrame(sockets[3], {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: backedOffReplay.request_id,
      action: "message.send",
      result: { event: event(1), event_seq: 1 },
      deduplicated: true,
    });
    await expect(pendingCommand).resolves.toMatchObject({
      accepted: true,
      resolution: "committed",
      deduplicated: true,
    });
    handle.close();
  });

  it("ends one command after eight unresolved replies without closing a healthy socket", async () => {
    vi.useFakeTimers();
    const { handle, sockets } = openHarness();
    await flushPromises();
    await openReadyConnection(0, handle, sockets);
    const pendingCommand = handle.command("message.send", { content: "bounded uncertainty" });
    void pendingCommand.catch(() => {});
    let exactCommand: ReturnType<typeof sentClientFrame> | undefined;

    for (let reply = 0; reply < 8; reply += 1) {
      await vi.waitFor(() => expect(sockets[reply].sent).toHaveLength(2));
      const command = sentClientFrame(sockets[reply]);
      exactCommand ||= command;
      expect(command).toEqual(exactCommand);
      receiveServerFrame(sockets[reply], unresolved(command));
      if (reply === 7) break;

      await vi.waitFor(() => expect(sockets[reply].readyState).toBe(WebSocket.CLOSED));
      await vi.advanceTimersByTimeAsync(500);
      await openReadyConnection(reply + 1, handle, sockets);
      await vi.advanceTimersByTimeAsync(Math.max(0, UNRESOLVED_DELAYS_MS[reply] - 500));
    }

    await expect(pendingCommand).rejects.toMatchObject({ category: "outcome_unknown" });
    expect(sockets[7].readyState).toBe(WebSocket.OPEN);
    expect(sockets).toHaveLength(8);
    handle.close();
  });

  it("ends one command after eight ACK deadlines and restores the room without replay", async () => {
    vi.useFakeTimers();
    const { handle, sockets } = openHarness();
    await flushPromises();
    await openReadyConnection(0, handle, sockets);
    const pendingCommand = handle.command("message.send", { content: "silent outcome" });
    void pendingCommand.catch(() => {});
    let exactCommand: ReturnType<typeof sentClientFrame> | undefined;

    for (let attempt = 0; attempt < 8; attempt += 1) {
      await vi.waitFor(() => expect(sockets[attempt].sent).toHaveLength(2));
      const command = sentClientFrame(sockets[attempt]);
      exactCommand ||= command;
      expect(command).toEqual(exactCommand);
      await vi.advanceTimersByTimeAsync(COMMAND_TIMEOUT_MS);
      await vi.waitFor(() => expect(sockets[attempt].readyState).toBe(WebSocket.CLOSED));
      if (attempt === 7) break;

      await vi.advanceTimersByTimeAsync(500);
      await openReadyConnection(attempt + 1, handle, sockets);
      await vi.advanceTimersByTimeAsync(Math.max(0, UNRESOLVED_DELAYS_MS[attempt] - 500));
    }

    await expect(pendingCommand).rejects.toMatchObject({ category: "outcome_unknown" });
    await vi.advanceTimersByTimeAsync(500);
    const finalFrames = await openReadyConnection(8, handle, sockets);
    expect(sockets[8].sent).toHaveLength(1);
    expect(finalFrames.receipt.room_id).toBe("general");
    handle.close();
  });
});

function unresolved(command: Record<string, unknown>) {
  return {
    op: "nack",
    accepted: false,
    resolution: "unresolved",
    request_id: command.request_id,
    action: "message.send",
    error: { code: "persistence_failed", message: "Persistence operation failed." },
  };
}

async function openReadyConnection(
  index: number,
  handle: ReturnType<typeof openHarness>["handle"],
  sockets: ReturnType<typeof openHarness>["sockets"]
) {
  expect(sockets).toHaveLength(index + 1);
  sockets[index].open();
  const frames = handshakeFrames(0, 0);
  sockets[index].receive(frames.receipt);
  sockets[index].receiveRaw(frames.rawSnapshot);
  await vi.waitFor(() => expect(handle.ready()).toBe(true));
  return frames;
}
