import { describe, expect, it, vi } from "vitest";
import { event, flushPromises, handshakeFrames, openHarness } from "./test/roomSocketHarness";
import { commandAckResultIsValid } from "./lib/roomSocketValidation";

const requestId = "00000000-0000-4000-8000-000000000001";
const resolution = { response_kind: "answers" as const, answers: { secret: ["synthetic-answer"] } };
const ack = {
  op: "ack", accepted: true, resolution: "committed", request_id: requestId,
  action: "provider.request.resolve", result: { provider_request_id: requestId, event_id: "resolved-event" },
};

describe("provider request response transport", () => {
  it("keeps another viewer's hidden request events in snapshot, live and history cursors", async () => {
    const hidden = (seq: number) => {
      const { content: _content, ...base } = event(seq);
      return { ...base, type: "event_hidden", visibility: "owner", actor: { participant_id: "", participant_type: "" } };
    };
    const onRoomSnapshot = vi.fn();
    const onRoomEvents = vi.fn();
    const onError = vi.fn();
    const { handle, sockets } = openHarness({ onRoomSnapshot, onRoomEvents, onError });
    try {
      await flushPromises(); sockets[0].open();
      const first = handshakeFrames(2, 2);
      sockets[0].receive(first.receipt);
      sockets[0].receive({ ...first.snap, events: [event(1), hidden(2)] });
      await flushPromises();
      expect(onError).not.toHaveBeenCalled();
      expect(handle.ready()).toBe(true);
      expect(onRoomSnapshot).toHaveBeenCalledOnce();
      sockets[0].receive({ op: "event", stream: "room_events", events: [hidden(3), event(4)], latest_seq: 4 });
      await flushPromises();
      expect(onRoomEvents).toHaveBeenCalledWith([hidden(3), event(4)]);
      expect(commandAckResultIsValid("room.history", { before_seq: 4, limit: 3 }, {
        events: [event(1), hidden(2), hidden(3)], oldest_seq: 1, last_seq: 4, has_more_before: false,
      }, "general", "operator-local")).toBe(true);
      // Ordinary events still require their actual actor; hidden events cannot carry one.
      for (const invalid of [
        { ...event(5), actor: hidden(5).actor },
        { ...hidden(5), actor: event(5).actor },
      ]) expect(commandAckResultIsValid("room.history", { before_seq: 0, limit: 1 }, {
        events: [invalid], oldest_seq: 5, last_seq: 5, has_more_before: true,
      }, "general", "operator-local")).toBe(false);
    } finally { handle.close(); }
  });

  it("retains the original identity and answer across reconnect without overwriting a pending response", async () => {
    vi.useFakeTimers();
    const { handle, sockets, opened } = openHarness({});
    try {
      await flushPromises(); sockets[0].open();
      const first = handshakeFrames(0, 0);
      sockets[0].receive(first.receipt); sockets[0].receiveRaw(first.rawSnapshot); await opened;
      await expect(handle.command("provider.request.resolve", resolution)).rejects.toMatchObject({ category: "request_id_required" });
      const pending = handle.resolveProviderRequest(requestId, resolution);
      const sent = sockets[0].sent.at(-1);
      expect(sent).toEqual({ op: "command", request_id: requestId, action: "provider.request.resolve", payload: resolution });
      await expect(handle.resolveProviderRequest(requestId, { response_kind: "acknowledge" })).rejects.toMatchObject({ category: "request_id_conflict" });
      sockets[0].close();
      await vi.advanceTimersByTimeAsync(500); await flushPromises(); sockets[1].open();
      const next = handshakeFrames(0, 0);
      sockets[1].receive(next.receipt); sockets[1].receiveRaw(next.rawSnapshot);
      await vi.advanceTimersByTimeAsync(1_000); await flushPromises();
      expect(sockets[1].sent.filter((frame) => frame.op === "command")).toEqual([sent]);
      sockets[1].receive(ack);
      await expect(pending).resolves.toMatchObject(ack);
    } finally { handle.close(); vi.useRealTimers(); }
  });

  it("accepts only an exact request-bound metadata receipt", () => {
    const valid = (result: unknown, id?: string) => commandAckResultIsValid(
      "provider.request.resolve", resolution, result, "general", "operator-local", id
    );
    expect(valid(ack.result, requestId)).toBe(true);
    expect(valid(ack.result)).toBe(false);
    expect(valid(ack.result, "another-request")).toBe(false);
    expect(valid({ ...ack.result, event_id: "" }, requestId)).toBe(false);
    expect(valid({ ...ack.result, answers: resolution.answers }, requestId)).toBe(false);
  });
});
