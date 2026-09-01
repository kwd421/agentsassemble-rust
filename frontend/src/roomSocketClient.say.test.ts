import { afterEach, describe, expect, it, vi } from "vitest";
import type { LobbyAttachmentRef } from "./api/messageAttachments";
import { RoomSocketSayError } from "./roomSocketClient";
import {
  event,
  flushPromises,
  handshakeFrames,
  openHarness,
  receiveServerFrame,
  sentClientFrame,
} from "./test/roomSocketHarness";

function attachment(hex: string): LobbyAttachmentRef {
  const id = `ma_${hex.repeat(32)}`;
  return {
    id,
    filename: `${hex}.txt`,
    content_type: "text/plain",
    size: 1,
    is_image: false,
    url: `/api/attachments/${id}?view=1`,
    download_url: `/api/attachments/${id}?download=1`,
  };
}

afterEach(() => {
  vi.restoreAllMocks();
  vi.useRealTimers();
});

describe("room socket message attachments", () => {
  it("signs exact ordered attachment IDs for text and attachment-only sends", async () => {
    const { handle, sockets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));

    const first = attachment("a");
    const second = attachment("b");
    const textSend = handle.say({
      message: "evidence",
      attachments: [second, first],
    });
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const textCommand = await sentClientFrame(sockets[0], frames);
    expect(textCommand).toMatchObject({
      action: "message.send",
      payload: {
        content: "evidence",
        attachment_ids: [second.id, first.id],
      },
    });
    await receiveServerFrame(sockets[0], frames, {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: textCommand.request_id,
      action: "message.send",
      result: { event: event(1), event_seq: 1 },
    });
    await expect(textSend).resolves.toEqual({ events: [] });

    const attachmentOnly = handle.say({ message: "", attachments: [first] });
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(3));
    const attachmentCommand = await sentClientFrame(sockets[0], frames, 2);
    expect(attachmentCommand).toMatchObject({
      action: "message.send",
      payload: { content: "", attachment_ids: [first.id] },
    });
    await receiveServerFrame(sockets[0], frames, {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: attachmentCommand.request_id,
      action: "message.send",
      result: { event: event(2), event_seq: 2 },
    });
    await expect(attachmentOnly).resolves.toEqual({ events: [] });
    handle.close();
  });

  it("rejects invalid attachment lists before signing or sending a command", async () => {
    const { handle, sockets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));
    const valid = attachment("a");

    for (const attachments of [
      [valid, valid],
      [{ ...valid, id: "ma_invalid" }],
      Array.from({ length: 9 }, (_, index) => attachment(index.toString(16))),
    ]) {
      await expect(handle.say({ message: "blocked", attachments })).rejects.toBeInstanceOf(
        RoomSocketSayError
      );
    }
    expect(sockets[0].sent).toHaveLength(1);
    handle.close();
  });

  it("signs each vote operation with its exact Rust-owned payload", async () => {
    const { handle, sockets } = openHarness();
    await flushPromises();
    sockets[0].open();
    const frames = await handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt);
    sockets[0].receiveRaw(frames.rawSnapshot);
    await vi.waitFor(() => expect(handle.ready()).toBe(true));

    const create = handle.say({
      message: "",
      attachments: [attachment("a")],
      kind: "vote",
      voteQuestion: "Ship it?",
      voteOptions: ["Yes", "No"],
      voteDurationSeconds: 300,
    });
    await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(2));
    const createCommand = await sentClientFrame(sockets[0], frames);
    expect(createCommand.payload).toEqual({
      kind: "vote",
      vote_question: "Ship it?",
      vote_options: ["Yes", "No"],
      vote_duration_seconds: 300,
      attachment_ids: [attachment("a").id],
    });
    await receiveServerFrame(sockets[0], frames, {
      op: "ack",
      accepted: true,
      resolution: "committed",
      request_id: createCommand.request_id,
      action: "message.send",
      result: { event: event(1), event_seq: 1 },
    });
    await expect(create).resolves.toEqual({ events: [] });

    const operations = [
      {
        request: {
          message: "",
          kind: "vote_cast" as const,
          voteId: "evt-1",
          voteChoice: "Yes",
        },
        payload: { kind: "vote_cast", vote_id: "evt-1", vote_choice: "Yes" },
      },
      {
        request: { message: "", kind: "vote_withdraw" as const, voteId: "evt-1" },
        payload: { kind: "vote_withdraw", vote_id: "evt-1" },
      },
      {
        request: { message: "", kind: "vote_close" as const, voteId: "evt-1" },
        payload: { kind: "vote_close", vote_id: "evt-1" },
      },
    ];
    for (const [index, operation] of operations.entries()) {
      const pending = handle.say(operation.request);
      const wireIndex = index + 2;
      await vi.waitFor(() => expect(sockets[0].sent).toHaveLength(wireIndex + 1));
      const command = await sentClientFrame(sockets[0], frames, wireIndex);
      expect(command.payload).toEqual(operation.payload);
      await receiveServerFrame(sockets[0], frames, {
        op: "ack",
        accepted: true,
        resolution: "committed",
        request_id: command.request_id,
        action: "message.send",
        result: { event: event(wireIndex), event_seq: wireIndex },
      });
      await expect(pending).resolves.toEqual({ events: [] });
    }
    handle.close();
  });
});
