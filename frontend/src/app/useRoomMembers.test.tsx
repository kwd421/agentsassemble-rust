import { renderHook } from "@testing-library/react";
import { Hash } from "lucide-react";
import { describe, expect, it } from "vitest";
import type { RoomMember } from "../api";
import type { RoomDockItem } from "../lib/roomDockModel";
import { useRoomMembers } from "./useRoomMembers";

const room: RoomDockItem = {
  id: "room-a",
  label: "Room A",
  meetingId: "meeting-a",
  topic: "A",
  shortLabel: "A",
  icon: Hash,
  createdAt: "2026-08-25T00:00:00Z",
  tone: "fresh",
};

const participant: RoomMember = {
  meeting_id: room.meetingId,
  participant_id: "operator-local",
  display_name: "Canonical Operator",
  role: "human",
  participant_type: "human",
  provider_kind: "manual",
  connection_kind: "browser",
  status: "joined",
  source: "test",
  created_at: "2026-08-25T00:00:00Z",
  updated_at: "2026-08-25T00:00:00Z",
};

describe("useRoomMembers", () => {
  it("projects only the active room's canonical WebSocket participants", () => {
    const hook = renderHook(() =>
      useRoomMembers({
        activeRoom: room,
        canonicalParticipants: [participant],
      })
    );

    expect(hook.result.current.activeMembers).toEqual([participant]);
    expect(hook.result.current.membersFor(room)).toEqual([participant]);
    expect(
      hook.result.current.membersFor({ ...room, id: "room-b", meetingId: "meeting-b" })
    ).toEqual([]);
  });

  it("exposes no participant projection while the canonical stream is disabled", () => {
    const hook = renderHook(() =>
      useRoomMembers({
        activeRoom: room,
        canonicalParticipants: [participant],
        enabled: false,
      })
    );

    expect(hook.result.current.activeMembers).toEqual([]);
    expect(hook.result.current.membersFor(room)).toEqual([]);
  });
});
