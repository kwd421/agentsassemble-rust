import { describe, expect, it } from "vitest";

import type { RoomEvent } from "../api";
import { applyParticipantEvents } from "./canonicalRoomProjection";

function joinedEvent(): RoomEvent {
  return {
    v: 1,
    id: "event-1",
    seq: 1,
    created_at: "2026-08-25T00:00:00Z",
    room_id: "general",
    type: "participant_joined",
    actor: { participant_id: "operator-local", participant_type: "human" },
    participant_id: "agent-one",
    participant_type: "agent",
    participant: {
      room_id: "general",
      participant_id: "agent-one",
      display_name: "Codex",
      avatar_image_url: "",
      participant_type: "subscription_ai",
      status: "joined",
      role: "agent",
      owner_id: "operator-local-user",
      muted: false,
      created_at: "2026-08-25T00:00:00Z",
      updated_at: "2026-08-25T00:00:00Z",
    },
  } as RoomEvent;
}

describe("canonical participant event projection", () => {
  it("inserts a newly joined participant from the complete sequenced event", () => {
    expect(applyParticipantEvents([], [joinedEvent()])).toEqual([
      expect.objectContaining({
        meeting_id: "general",
        participant_id: "agent-one",
        display_name: "Codex",
        status: "joined",
        source: "agent_session",
      }),
    ]);
  });

  it("rejects a joined event that does not carry its room-owned participant state", () => {
    const malformed = { ...joinedEvent(), participant: undefined } as RoomEvent;
    expect(() => applyParticipantEvents([], [malformed])).toThrow(/참가자 투영/);
  });

  it("rejects role aliases instead of inventing frontend compatibility", () => {
    const original = joinedEvent() as RoomEvent & {
      participant: Record<string, unknown>;
    };
    const malformed = {
      ...original,
      participant: { ...original.participant, role: "host" },
    } as RoomEvent;
    expect(() => applyParticipantEvents([], [malformed])).toThrow(/참가자 투영/);
  });

  it("derives presentation source from participant kind rather than room role", () => {
    const original = joinedEvent() as RoomEvent & {
      participant: Record<string, unknown>;
    };
    const humanDirector = {
      ...original,
      participant_id: "human-one",
      participant_type: "human",
      participant: {
        ...original.participant,
        participant_id: "human-one",
        participant_type: "human",
        role: "director",
      },
    } as RoomEvent;

    expect(applyParticipantEvents([], [humanDirector])).toEqual([
      expect.objectContaining({
        participant_id: "human-one",
        participant_type: "human",
        role: "director",
        source: "room",
      }),
    ]);
  });

  it("projects canonical mute events without changing Agent profile fields", () => {
    const joined = applyParticipantEvents([], [joinedEvent()]);
    const muted = applyParticipantEvents(joined, [
      {
        v: 1,
        id: "event-2",
        seq: 2,
        created_at: "2026-08-25T00:00:01Z",
        room_id: "general",
        type: "participant_muted",
        actor: { participant_id: "operator-local", participant_type: "human" },
        participant_id: "agent-one",
        participant_type: "agent",
        muted: true,
      } as RoomEvent,
    ]);

    expect(muted).toEqual([
      expect.objectContaining({
        participant_id: "agent-one",
        display_name: "Codex",
        role: "agent",
        muted: true,
      }),
    ]);
  });
});
