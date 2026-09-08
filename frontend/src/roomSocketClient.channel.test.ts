import { describe, expect, it, vi } from "vitest";
import { flushPromises, handshakeFrames, openHarness } from "./test/roomSocketHarness";
import { channelId, channelMessage } from "./test/channelMessage";
import { commandAckResultIsValid, publicRoomEventIsValid } from "./lib/roomSocketValidation";
import { projectRoomEventsToTimeline } from "./lib/roomEventProjection";

describe("custom-channel wire projection", () => {
  it("delivers a channel event and exact send receipt without adding a lobby message", async () => {
    const onRoomEvents = vi.fn();
    const { handle, sockets, opened } = openHarness({ onRoomEvents });
    await flushPromises(); sockets[0].open();
    const frames = handshakeFrames(0, 0);
    sockets[0].receive(frames.receipt); sockets[0].receiveRaw(frames.rawSnapshot); await opened;
    const event = channelMessage(1);
    const pending = handle.command("channel.message.send", { channel_id: channelId, content: "message 1" });
    const request = sockets[0].sent.at(-1)!;
    sockets[0].receive({ op: "ack", accepted: true, resolution: "committed", request_id: request.request_id, action: "channel.message.send", result: { channel_id: channelId, event, event_seq: 1 } });
    await expect(pending).resolves.toMatchObject({ result: { event } });
    sockets[0].receive({ op: "event", stream: "room_events", events: [event], latest_seq: 1 });
    await flushPromises();
    expect(onRoomEvents).toHaveBeenCalledWith([event]);
    expect(projectRoomEventsToTimeline([event])).toEqual([]);
    handle.close();
  });

  it("does not replay a pending channel send into a recreated namesake room", async () => {
    vi.useFakeTimers();
    const onRoomSnapshot = vi.fn();
    const { handle, sockets, opened } = openHarness({ onRoomSnapshot });
    try {
      await flushPromises(); sockets[0].open();
      const first = handshakeFrames(3, 3);
      sockets[0].receive(first.receipt); sockets[0].receiveRaw(first.rawSnapshot); await opened;
      const pending = handle.command("channel.message.send", { channel_id: channelId, content: "old-room message" }).catch((error: unknown) => error);
      sockets[0].close();
      await vi.advanceTimersByTimeAsync(500); await flushPromises(); sockets[1].open();
      const next = handshakeFrames(4, 4);
      next.snap.room.room_uid = "00000000-0000-4000-8000-000000000002";
      sockets[1].receive(next.receipt); sockets[1].receiveRaw(JSON.stringify(next.snap));
      await flushPromises();
      expect(sockets[1].sent.filter((frame) => frame.op === "command")).toEqual([]);
      expect(onRoomSnapshot).toHaveBeenCalledTimes(1);
      await vi.advanceTimersByTimeAsync(1_000); await flushPromises(); sockets[2].open();
      expect(sockets[2].sent[0]).toMatchObject({ resume_from_seq: 0 });
      const fresh = handshakeFrames(0, 0);
      fresh.snap.room.room_uid = next.snap.room.room_uid;
      sockets[2].receive(fresh.receipt); sockets[2].receiveRaw(JSON.stringify(fresh.snap));
      await flushPromises();
      expect(handle.ready()).toBe(true);
      expect(onRoomSnapshot).toHaveBeenCalledTimes(2);
      expect(sockets[2].sent.filter((frame) => frame.op === "command")).toEqual([]);
      handle.close();
      await expect(pending).resolves.toMatchObject({ category: "outcome_unknown" });
    } finally { handle.close(); vi.useRealTimers(); }
  });

  it("rejects cross-channel ACKs, false lobby records and malformed page boundaries", () => {
    const event = channelMessage(4);
    expect(publicRoomEventIsValid({ ...event, actor_id: "other" }, "general")).toBe(false);
    expect(publicRoomEventIsValid({ ...event, attachments: [] }, "general")).toBe(false);
    expect(commandAckResultIsValid("channel.message.send", { channel_id: "c111111111111" }, { channel_id: channelId, event, event_seq: 4 }, "general", "operator-local")).toBe(false);
    const retired = { ...event, content: "", message_deleted: true };
    expect(publicRoomEventIsValid(retired, "general")).toBe(true);
    expect(commandAckResultIsValid("channel.message.send", { channel_id: channelId }, { channel_id: channelId, event: retired, event_seq: 4 }, "general", "operator-local")).toBe(false);
    const payload = { channel_id: channelId, before_seq: 8, limit: 80 };
    const page = { room_id: "general", channel_id: channelId, events: [channelMessage(2), event], oldest_seq: 2, last_seq: 9, has_more_before: true };
    const valid = (value: unknown) => commandAckResultIsValid("channel.history", payload, value, "general", "operator-local");
    expect(valid(page)).toBe(true);
    for (const malformed of [
      { ...page, room_id: "other" }, { ...page, channel_id: "c111111111111" },
      { ...page, oldest_seq: 1 }, { ...page, events: [channelMessage(8)] },
      { ...page, events: [event, event] }, { ...page, last_seq: 3 },
      { ...page, events: [{ ...event, type: "message_final" }] },
      { ...page, events: [retired], oldest_seq: 4 },
    ]) expect(valid(malformed)).toBe(false);
    expect(valid({ ...page, events: [], oldest_seq: 0, has_more_before: false })).toBe(true);
  });
});
