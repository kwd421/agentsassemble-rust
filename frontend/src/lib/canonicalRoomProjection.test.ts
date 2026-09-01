import { describe, expect, it } from "vitest";

import type { LobbyEvent, RoomAgentSession, RoomEvent, RoomMember } from "../api";
import {
  applyCanonicalParticipantProfiles,
  applyParticipantEvents,
  canonicalParticipantProfiles,
  mergeRoomEvents,
} from "./canonicalRoomProjection";

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
  it("retains only the server-sized live event window", () => {
    const events = Array.from({ length: 201 }, (_, index) => ({
      ...joinedEvent(),
      id: `event-${index + 1}`,
      seq: index + 1,
    }));

    const retained = mergeRoomEvents([], events, false);

    expect(retained).toHaveLength(200);
    expect(retained[0].seq).toBe(2);
    expect(retained.at(-1)?.seq).toBe(201);
  });

  it("reapplies the current participant profile to an older displayed page", () => {
    const older: LobbyEvent = {
      id: "older-message",
      record_id: "older-record",
      kind: "message",
      name: "Old name",
      message: "hello",
      side: "other",
      created_at: "2026-08-25T00:00:00Z",
      actor_id: "agent-one",
    };

    expect(applyCanonicalParticipantProfiles([older], {
      "agent-one": {
        displayName: "Current name",
        avatarImageUrl: "http://127.0.0.1/avatar",
        providerKind: "codex_live_session",
        role: "agent",
      },
    })).toEqual([
      expect.objectContaining({
        id: "older-message",
        name: "Current name",
        avatar_image_url: "http://127.0.0.1/avatar",
      }),
    ]);
  });

  it("projects Agent identity from its session and room role from its participant", () => {
    const participant = {
      participant_id: "agent-one",
      display_name: "stale participant name",
      avatar_image_url: "/api/attachments/stale-avatar?view=1",
      provider_kind: "stale-provider",
      role: "reviewer",
    } as RoomMember;
    const session = {
      participant_id: "agent-one",
      display_name: "Session identity",
      avatar_image_url: "/api/attachments/agent-avatar?view=1",
      provider_kind: "codex_live_session",
    } as RoomAgentSession;

    expect(
      canonicalParticipantProfiles(
        [session],
        [participant],
        "http://127.0.0.1:8080",
      ),
    ).toEqual({
      "agent-one": {
        displayName: "Session identity",
        avatarImageUrl:
          "http://127.0.0.1:8080/api/attachments/agent-avatar?view=1",
        providerKind: "codex_live_session",
        role: "reviewer",
      },
    });
  });

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
