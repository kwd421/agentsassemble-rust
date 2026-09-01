import { afterEach, describe, expect, it, vi } from "vitest";
import {
  event,
  flushPromises,
  handshakeFrames,
  openHarness,
  receiveAuthenticated,
  sentAuthenticatedCommand,
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
    const { handle, sockets, tickets } = openHarness();
    await flushPromises();
    let frames = await openReadyConnection(0, handle, sockets, tickets);
    const pendingCommand = handle.command("message.send", { content: "received before close" });
    void pendingCommand.catch(() => {});

    for (let attempt = 0; attempt < 7; attempt += 1) {
      await vi.waitFor(() => expect(sockets[attempt].sent).toHaveLength(2));
      const command = await sentAuthenticatedCommand(sockets[attempt], frames);
      await receiveAuthenticated(sockets[attempt], frames, unresolved(command));
      await vi.waitFor(() => expect(sockets[attempt].readyState).toBe(WebSocket.CLOSED));
      await vi.advanceTimersByTimeAsync(500);
      frames = await openReadyConnection(attempt + 1, handle, sockets, tickets);
      await vi.advanceTimersByTimeAsync(Math.max(0, UNRESOLVED_DELAYS_MS[attempt] - 500));
    }

    await vi.waitFor(() => expect(sockets[7].sent).toHaveLength(2));
    const finalCommand = await sentAuthenticatedCommand(sockets[7], frames);
    await receiveAuthenticated(sockets[7], frames, {
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
    const { handle, sockets, tickets } = openHarness();
    await flushPromises();
    const firstFrames = await openReadyConnection(0, handle, sockets, tickets);
    const pendingCommand = handle.command("message.send", { content: "closed transport" });
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const firstCommand = await sentAuthenticatedCommand(sockets[0], firstFrames);

    sockets[0].close();
    await vi.advanceTimersByTimeAsync(500);
    const secondFrames = await openReadyConnection(1, handle, sockets, tickets);
    await vi.waitFor(() => expect(sockets[1].sent).toHaveLength(2));
    const replayed = await sentAuthenticatedCommand(sockets[1], secondFrames);
    expect(replayed).toEqual(firstCommand);
    await receiveAuthenticated(sockets[1], secondFrames, {
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
    const { handle, sockets, tickets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const firstFrames = await handshakeFrames(sockets[0], tickets[0], 0, 0);
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
    const firstCommand = await sentAuthenticatedCommand(sockets[0], firstFrames);
    commandPayload.content = "mutated after send";

    await vi.advanceTimersByTimeAsync(COMMAND_TIMEOUT_MS);
    expect(sockets[0].readyState).toBe(WebSocket.CLOSED);
    expect(settled).toBe(false);

    await vi.advanceTimersByTimeAsync(500);
    sockets[1].open();
    const secondFrames = await handshakeFrames(sockets[1], tickets[1], 0, 0);
    sockets[1].receive(secondFrames.receipt);
    sockets[1].receiveRaw(secondFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    await vi.waitFor(() => expect(sockets[1].sent).toHaveLength(2));
    const replayedCommand = await sentAuthenticatedCommand(sockets[1], secondFrames);
    expect(replayedCommand).toEqual(firstCommand);

    await receiveAuthenticated(sockets[1], secondFrames, unresolved(replayedCommand));
    await vi.waitFor(() => expect(sockets[1].readyState).toBe(WebSocket.CLOSED));
    expect(settled).toBe(false);

    await vi.advanceTimersByTimeAsync(500);
    sockets[2].open();
    const thirdFrames = await handshakeFrames(sockets[2], tickets[2], 0, 0);
    sockets[2].receive(thirdFrames.receipt);
    sockets[2].receiveRaw(thirdFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    await vi.waitFor(() => expect(sockets[2].sent).toHaveLength(2));
    const replayedAfterUnresolved = await sentAuthenticatedCommand(sockets[2], thirdFrames);
    expect(replayedAfterUnresolved).toEqual(firstCommand);

    await receiveAuthenticated(sockets[2], thirdFrames, unresolved(replayedAfterUnresolved));
    await vi.waitFor(() => expect(sockets[2].readyState).toBe(WebSocket.CLOSED));
    await vi.advanceTimersByTimeAsync(500);
    sockets[3].open();
    const fourthFrames = await handshakeFrames(sockets[3], tickets[3], 0, 0);
    sockets[3].receive(fourthFrames.receipt);
    sockets[3].receiveRaw(fourthFrames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    expect(sockets[3].sent).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(500);
    await vi.waitFor(() => expect(sockets[3].sent).toHaveLength(2));
    const backedOffReplay = await sentAuthenticatedCommand(sockets[3], fourthFrames);
    expect(backedOffReplay).toEqual(firstCommand);

    await receiveAuthenticated(sockets[3], fourthFrames, {
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
    const { handle, sockets, tickets } = openHarness();
    await flushPromises();
    let frames = await openReadyConnection(0, handle, sockets, tickets);
    const pendingCommand = handle.command("message.send", { content: "bounded uncertainty" });
    void pendingCommand.catch(() => {});
    let exactCommand: Awaited<ReturnType<typeof sentAuthenticatedCommand>> | undefined;

    for (let reply = 0; reply < 8; reply += 1) {
      await vi.waitFor(() => expect(sockets[reply].sent).toHaveLength(2));
      const command = await sentAuthenticatedCommand(sockets[reply], frames);
      exactCommand ||= command;
      expect(command).toEqual(exactCommand);
      await receiveAuthenticated(sockets[reply], frames, unresolved(command));
      if (reply === 7) break;

      await vi.waitFor(() => expect(sockets[reply].readyState).toBe(WebSocket.CLOSED));
      await vi.advanceTimersByTimeAsync(500);
      frames = await openReadyConnection(reply + 1, handle, sockets, tickets);
      await vi.advanceTimersByTimeAsync(Math.max(0, UNRESOLVED_DELAYS_MS[reply] - 500));
    }

    await expect(pendingCommand).rejects.toMatchObject({ category: "outcome_unknown" });
    expect(sockets[7].readyState).toBe(WebSocket.OPEN);
    expect(sockets).toHaveLength(8);
    handle.close();
  });

  it("ends one command after eight ACK deadlines and restores the room without replay", async () => {
    vi.useFakeTimers();
    const { handle, sockets, tickets } = openHarness();
    await flushPromises();
    let frames = await openReadyConnection(0, handle, sockets, tickets);
    const pendingCommand = handle.command("message.send", { content: "silent outcome" });
    void pendingCommand.catch(() => {});
    let exactCommand: Awaited<ReturnType<typeof sentAuthenticatedCommand>> | undefined;

    for (let attempt = 0; attempt < 8; attempt += 1) {
      await vi.waitFor(() => expect(sockets[attempt].sent).toHaveLength(2));
      const command = await sentAuthenticatedCommand(sockets[attempt], frames);
      exactCommand ||= command;
      expect(command).toEqual(exactCommand);
      await vi.advanceTimersByTimeAsync(COMMAND_TIMEOUT_MS);
      await vi.waitFor(() => expect(sockets[attempt].readyState).toBe(WebSocket.CLOSED));
      if (attempt === 7) break;

      await vi.advanceTimersByTimeAsync(500);
      frames = await openReadyConnection(attempt + 1, handle, sockets, tickets);
      await vi.advanceTimersByTimeAsync(Math.max(0, UNRESOLVED_DELAYS_MS[attempt] - 500));
    }

    await expect(pendingCommand).rejects.toMatchObject({ category: "outcome_unknown" });
    await vi.advanceTimersByTimeAsync(500);
    const finalFrames = await openReadyConnection(8, handle, sockets, tickets);
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
  sockets: ReturnType<typeof openHarness>["sockets"],
  tickets: ReturnType<typeof openHarness>["tickets"]
) {
  expect(sockets).toHaveLength(index + 1);
  sockets[index].open();
  const frames = await handshakeFrames(sockets[index], tickets[index], 0, 0);
  sockets[index].receive(frames.receipt);
  sockets[index].receiveRaw(frames.rawSnapshot);
  await vi.waitFor(() => expect(handle.ready()).toBe(true));
  return frames;
}
