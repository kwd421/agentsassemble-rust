import { describe, expect, it } from "vitest";
import type { RoomEvent } from "../api";
import { participantFixture } from "../test/participant";
import { applyParticipantEvents, normalizeActiveRoomParticipants } from "./canonicalRoomProjection";
import { commandAckResultIsValid, publicRoomEventIsValid } from "./roomSocketValidation";

describe("canonical participant removal", () => {
  it.each(["kick", "export"] as const)("projects %s and rejects mismatched ACK authority", (action) => {
    const participant = participantFixture({ participant_id: "guest", status: action === "kick" ? "kicked" : "exported" });
    const event = {
      v: 1, id: "removed", seq: 3, created_at: participant.updated_at,
      room_id: "general", type: action === "kick" ? "participant_kicked" : "participant_exported",
      actor: { participant_id: "operator-local", participant_type: "human" },
      participant_id: participant.participant_id, participant_type: participant.participant_type,
      display_name: participant.display_name, participant,
    } as RoomEvent;
    const payload = { participant_id: "guest" };
    const result = { participant, revoked_sessions: 1, cleanup_pending: false, event, event_seq: 3, events: [event] };
    const valid = (candidate: unknown) => commandAckResultIsValid(`participant.${action}`, payload, candidate, "general", "operator-local");
    expect(valid(result)).toBe(true);
    const records = applyParticipantEvents([participantFixture({ participant_id: "guest" })], [event]);
    expect(records).toEqual([participant]);
    expect(normalizeActiveRoomParticipants(records)).toEqual([]);
    expect(valid({ ...result, participant: { ...participant, participant_id: "other" } })).toBe(false);
    expect(valid({ ...result, participant: { ...participant, status: "joined" } })).toBe(false);
    expect(publicRoomEventIsValid({ ...event, room_id: "other-room" }, "general")).toBe(false);
    expect(publicRoomEventIsValid({ ...event, display_name: "different" }, "general")).toBe(false);
    expect(valid({ ...result, events: [{ ...event, participant: { ...participant, status: "joined" } }] })).toBe(false);
  });
});
