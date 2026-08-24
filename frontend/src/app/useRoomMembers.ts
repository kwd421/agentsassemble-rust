import { useCallback } from "react";
import type { RoomMember } from "../api";
import type { RoomDockItem } from "../lib/roomDockModel";

type UseRoomMembersOptions = {
  activeRoom: RoomDockItem;
  canonicalParticipants: RoomMember[];
  enabled?: boolean;
};

export function useRoomMembers({
  activeRoom,
  canonicalParticipants,
  enabled = true,
}: UseRoomMembersOptions) {
  const membersFor = useCallback(
    (room: RoomDockItem) =>
      enabled &&
      Boolean(activeRoom.meetingId) &&
      room.meetingId === activeRoom.meetingId
        ? canonicalParticipants
        : [],
    [activeRoom.meetingId, canonicalParticipants, enabled]
  );

  return {
    activeMembers: enabled ? canonicalParticipants : [],
    membersFor,
  };
}
