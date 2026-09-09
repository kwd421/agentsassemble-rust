import { describe, expect, it, vi } from "vitest";
import { commandAckResultIsValid, publicRoomEventIsValid } from "./lib/roomSocketValidation";
import { event, flushPromises, handshakeFrames, openHarness, sentClientFrame } from "./test/roomSocketHarness";

// The domain's privacy_minimized_vote_transition retains no ballot identity or choice.
const voteMarker = (seq: number, kind: string) => ({
  ...event(seq), actor: { participant_id: "", participant_type: "" },
  content: "", message_kind: kind, vote_id: "poll-1",
});

describe("anonymous vote marker transport", () => {
  it("recovers an existing ballot and accepts live transitions, receipts and history", async () => {
    const onRoomEvents = vi.fn();
    const onError = vi.fn();
    const { handle, sockets } = openHarness({ onRoomEvents, onError });
    const cast = voteMarker(1, "vote_cast");
    const withdraw = voteMarker(2, "vote_withdraw");
    const close = voteMarker(3, "vote_close");
    try {
      await flushPromises(); sockets[0].open();
      const frames = handshakeFrames(1, 1);
      sockets[0].receive(frames.receipt);
      sockets[0].receive({ ...frames.snap, events: [cast] });
      await flushPromises();
      expect(onError).not.toHaveBeenCalled();
      expect(handle.ready()).toBe(true);

      const pending = handle.say({ message: "", kind: "vote_withdraw", voteId: "poll-1" });
      await flushPromises();
      const command = sentClientFrame(sockets[0]);
      sockets[0].receive({ op: "event", stream: "room_events", events: [withdraw], latest_seq: 2 });
      sockets[0].receive({ op: "ack", accepted: true, resolution: "committed",
        request_id: command.request_id, action: "message.send", result: { event: withdraw, event_seq: 2 } });
      await expect(pending).resolves.toEqual({ events: [] });
      sockets[0].receive({ op: "event", stream: "room_events", events: [close], latest_seq: 3 });
      await flushPromises();
      expect(onRoomEvents).toHaveBeenCalledWith([withdraw]);
      expect(onRoomEvents).toHaveBeenCalledWith([close]);
      expect(onError).not.toHaveBeenCalled();
      expect(handle.ready()).toBe(true);
      expect(commandAckResultIsValid("room.history", { before_seq: 0, limit: 3 }, {
        events: [cast, withdraw, close], oldest_seq: 1, last_seq: 3, has_more_before: false,
      }, "general", "operator-local")).toBe(true);
    } finally { handle.close(); }
  });

  it("does not treat ordinary messages or malformed vote markers as anonymous events", () => {
    const cast = voteMarker(1, "vote_cast");
    for (const invalid of [
      { ...event(1), actor: cast.actor },
      { ...cast, actor: event(1).actor },
      { ...cast, actor: { participant_id: "", participant_type: "human" } },
      { ...cast, vote_id: "" },
      { ...cast, content: "a private ballot" },
      { ...cast, message_kind: "vote" },
    ]) expect(publicRoomEventIsValid(invalid, "general")).toBe(false);
  });
});
