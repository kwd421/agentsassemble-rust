import { useCallback, useEffect, type MutableRefObject } from "react";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { RoomEvent } from "../api";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import {
  isMembershipEndedTerminal,
  markMembershipEndedTerminal,
  membershipEndedScopeFromEventType,
  type MemberSelectError,
} from "../lib/memberMembershipEnded";
import { applyConfirmedMembershipLost } from "./remoteRoomAuthority";
import type { ChannelMenuState } from "./appModel";
import type { RoomMenuState } from "../views/components/RoomRail";

/**
 * Terminal kicked-room ownership: the viewer was removed from a joined
 * member room (or its select/heal was refused with member_membership_ended).
 * The ended view owns the acknowledgement — 확인 removes the room from the
 * rail through the existing removal owner with no retry — and the
 * session-loss heal / auto re-select treat the room as terminal afterwards.
 */
export function useMembershipEndedRoom({
  activeRoom,
  rooms,
  guestSession,
  guestExpired,
  memberSelectError,
  setMemberSelectError,
  removeDirectoryRoom,
  removeMemberRoom,
  clearGuestSession,
  retainedActiveRoom,
  setActiveRoomId,
  setChannel,
  setAdminOpen,
  setRoomMenu,
  setChannelMenu,
  viewerParticipantId,
  roomEvents,
}: {
  activeRoom: RoomDockItem;
  rooms: RoomDockItem[];
  guestSession: RoomGuestSession | null;
  guestExpired: boolean;
  memberSelectError: MemberSelectError | null;
  setMemberSelectError: (error: MemberSelectError | null | ((current: MemberSelectError | null) => MemberSelectError | null)) => void;
  removeDirectoryRoom: (roomId: string) => RoomDockItem[];
  removeMemberRoom: (serverId: string, roomId: string) => void;
  clearGuestSession: () => void;
  retainedActiveRoom: MutableRefObject<RoomDockItem | null>;
  setActiveRoomId: (roomId: string) => void;
  setChannel: (channel: "lobby") => void;
  setAdminOpen: (open: boolean) => void;
  setRoomMenu: (menu: RoomMenuState) => void;
  setChannelMenu: (menu: ChannelMenuState) => void;
  viewerParticipantId: string;
  roomEvents: RoomEvent[];
}) {
  const acknowledgeMembershipEnded = useCallback(() => {
    const error = memberSelectError;
    const room = (error && rooms.find((candidate) => candidate.id === error.roomId)) ?? activeRoom;
    const scope = error?.membershipEnded ?? "kicked";
    if (room.roomOrigin === "remote_server" && room.serverId) {
      markMembershipEndedTerminal(room.serverId, room.meetingId, scope);
      removeMemberRoom(room.serverId, room.meetingId);
      if (guestSession && !guestExpired &&
          guestSession.serverSurface.server_id === room.serverId &&
          guestSession.meetingId === room.meetingId) {
        clearGuestSession();
      }
    }
    setMemberSelectError(null);
    retainedActiveRoom.current = null;
    removeDirectoryRoom(room.id);
    const fallback = rooms.find((candidate) => candidate.id !== room.id);
    if (activeRoom.id === room.id) {
      setActiveRoomId(fallback?.id || "");
      setChannel("lobby");
      setAdminOpen(false);
    }
    setRoomMenu(null);
    setChannelMenu(null);
  }, [memberSelectError, rooms, activeRoom, guestSession, guestExpired,
    removeMemberRoom, clearGuestSession, setMemberSelectError,
    retainedActiveRoom, removeDirectoryRoom,
    setActiveRoomId, setChannel, setAdminOpen, setRoomMenu, setChannelMenu]);

  // The viewer was kicked out of the open room: the host's canonical
  // participant_kicked / participant_exported event for the viewer's own
  // participant parks this room as terminal and shows the ended view. The
  // session-loss heal and the auto re-select never retry it afterwards.
  useEffect(() => {
    if (activeRoom.roomOrigin !== "remote_server" || !activeRoom.serverId) return;
    if (memberSelectError && memberSelectError.roomId === activeRoom.id && memberSelectError.membershipEnded) return;
    const removed = roomEvents.find((event) =>
      (event.type === "participant_kicked" || event.type === "participant_exported") &&
      event.participant_id === viewerParticipantId &&
      event.room_id === activeRoom.meetingId);
    if (!removed) return;
    const scope = membershipEndedScopeFromEventType(removed.type);
    markMembershipEndedTerminal(activeRoom.serverId, activeRoom.meetingId, scope);
    if (!isMembershipEndedTerminal(activeRoom.serverId, activeRoom.meetingId)) return;
    setMemberSelectError((current) => current?.roomId === activeRoom.id ? current : {
      roomId: activeRoom.id, message: "방을 열지 못했어요. 다시 시도해 주세요.", membershipEnded: scope,
    });
  }, [roomEvents, activeRoom, memberSelectError, viewerParticipantId, setMemberSelectError]);

  return { acknowledgeMembershipEnded };
}

/**
 * Confirmed membership loss (kick/ban/revoke while central is connected):
 * the adopted channel closes here and the caller drops the server's rooms
 * from the rail. A temporary central outage never reaches this.
 */
export function createConfirmedMembershipLostHandler({
  guestSession, guestExpired, latestRooms, activeRoomId, memberSelectError,
  closeHost, unbindHostSessions, clearSession, removeRoom,
  setSelectError, activateRoom, resetChannel, closeAdmin, clearRetainedRoom,
}: {
  guestSession: RoomGuestSession | null;
  guestExpired: boolean;
  latestRooms: RoomDockItem[];
  activeRoomId: string;
  memberSelectError: MemberSelectError | null;
  closeHost: (serverId: string) => void;
  unbindHostSessions: (serverId: string) => void;
  clearSession: () => void;
  removeRoom: (roomId: string) => void;
  setSelectError: (error: MemberSelectError | null) => void;
  activateRoom: (roomId: string) => void;
  resetChannel: () => void;
  closeAdmin: () => void;
  clearRetainedRoom: () => void;
}) {
  return (serverId: string) => {
    applyConfirmedMembershipLost(serverId,
      { guestSession, guestExpired, latestRooms, activeRoomId, memberSelectError },
      { closeHost, unbindHostSessions, clearSession, removeRoom,
        setSelectError, activateRoom, resetChannel, closeAdmin, clearRetainedRoom });
  };
}
