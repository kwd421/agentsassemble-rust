import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { handshakeFrames, openHarness, flushPromises } from "./test/roomSocketHarness";
import { decideDelegatedAction, readDelegatedActionRequests } from "./lib/delegatedActionProjection";
import type { DelegatedActionRequest } from "./types/generated/DelegatedActionRequest";

const request: DelegatedActionRequest = {
  confirmation_id: "8b6be8f4-7315-4b6c-b4b6-ff12f57e69b4", owner_participant_id: "operator-local", owner_display_name: "Owner",
  delegate_participant_id: "ai", delegate_display_name: "Director", action: { action: "delete_text_channel", request_id: "8b6be8f4-7315-4b6c-b4b6-ff12f57e69b5", channel_id: "ctarget", expected_settings_revision: "rev" },
  target_name: "Planning", consequences: "Deletes messages", created_at: "2026-10-13T00:00:00Z", expires_at: "2026-10-13T00:05:00Z", state: "pending",
};
describe("owner confirmation socket projection", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());
  it("hydrates pending requests, applies terminal events and sends the exact decision identity", async () => {
    const { handle, sockets } = openHarness();
    await flushPromises();
    const socket = sockets[0];
    socket.open();
    const frames = handshakeFrames(0, 0);
    socket.receive(frames.receipt); socket.receive(frames.catalog); socket.receive(frames.requestsEnd);
    socket.receive({ ...frames.snap, delegated_action_requests: [request] });
    expect(readDelegatedActionRequests(handle)).toEqual([request]);
    const decided = decideDelegatedAction(handle, { confirmation_id: request.confirmation_id, decision: "decline" });
    expect(socket.sent.at(-1)).toMatchObject({ op: "command", request_id: request.confirmation_id, action: "delegated_action.decide", payload: { confirmation_id: request.confirmation_id, decision: "decline" } });
    socket.receive({ op: "ack", request_id: request.confirmation_id, accepted: true, resolution: "committed", action: "delegated_action.decide", result: { confirmation_id: request.confirmation_id, state: "declined", receipt: { status: "rejected", code: "confirmation_declined" } } });
    await expect(decided).resolves.toMatchObject({ state: "declined" });
    socket.receive({ op: "event", stream: "room_events", latest_seq: 1, events: [{ v: 1, id: "event", room_id: "general", seq: 1, created_at: request.created_at, type: "delegated_action_updated", actor: { participant_id: "ai", participant_type: "agent" }, visibility: "owner", owner_id: request.owner_participant_id, delegated_action_request: { ...request, state: "declined" } }] });
    expect(readDelegatedActionRequests(handle)).toEqual([]);
    handle.close();
  });
});
