import type { RoomSocketAuth } from "../api/room";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { MemberSelectError } from "../lib/memberMembershipEnded";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import type { ServerProductSurface } from "../types/generated/ServerProductSurface";
import type { RoomStream } from "../types/generated/RoomStream";
import type { RoomPreferenceAuthority } from "./useRoomSettingsController";

// Stable empty stream list for rooms without a server surface (a remote room
// without its member session). The socket effect depends on this identity, so
// a fresh array per render would reopen it in a loop.
export const NO_SURFACE_STREAMS: RoomStream[] = [];

// An anchored member room needs its own room session before it can open:
// only the member select/admission path may make it live. Every other
// activation path either routes through it or leaves the room disconnected.
export function memberAdmissionServerFor(
  room: Pick<RoomDockItem, "roomOrigin" | "serverId">,
  ownerServerId: string | undefined,
  hasAnchor: (serverId: string) => boolean,
): string | null {
  return room.roomOrigin === "remote_server" && room.serverId &&
    room.serverId !== ownerServerId && hasAnchor(room.serverId)
    ? room.serverId : null;
}

// Host auth of this computer never drives a remote host's room: without that
// room's own member session the room stays disconnected (no socket, no host
// ticket), even while its host lists it as connected.
export function remoteRoomSessionReady(
  room: Pick<RoomDockItem, "roomOrigin">,
  member: { guestLocked: boolean; sessionToken: string; session: RoomGuestSession | null; expired: boolean },
): boolean {
  if (room.roomOrigin !== "remote_server") return true;
  return Boolean(member.guestLocked && member.sessionToken && member.session && !member.expired);
}

// The active room opens with its own owner's surface (this computer's for own
// rooms, the matching member session's for its room). Without a session a
// remote room has no surface: nothing validates or drives it.
export function serverProductSurfaceFor(
  sessionReady: boolean,
  activeSession: RoomGuestSession | null,
  guestSession: RoomGuestSession | null,
  scopedToRoom: boolean,
  hostSurface: ServerProductSurface | null | undefined,
): ServerProductSurface | null | undefined {
  if (!sessionReady) return undefined;
  if (activeSession) return activeSession.serverSurface.server_product_surface;
  if (guestSession && !scopedToRoom) return guestSession.serverSurface.server_product_surface;
  return hostSurface;
}

// Like the surface: only a room's own session authorizes it, so a remote
// room without its session resolves no host ticket at all.
export function canonicalRoomAuthFor(
  room: Pick<RoomDockItem, "meetingId">,
  ready: { operational: boolean; selectError: boolean; guestLocked: boolean },
  sessionToken: string,
  deviceToken: string,
): RoomSocketAuth | undefined {
  if (ready.operational || ready.selectError) return undefined;
  if (ready.guestLocked) {
    return sessionToken
      ? ({ kind: "session", sessionToken, deviceToken })
      : undefined;
  }
  return room.meetingId ? ({ kind: "host", meetingId: room.meetingId }) : undefined;
}

// Room preferences follow the same authority: a session-less remote room can
// neither read nor write through this computer.
export function roomPreferenceAuthorityFor(
  sessionReady: boolean,
  guestLocked: boolean,
  sessionToken: string,
  deviceToken: string,
): RoomPreferenceAuthority {
  if (!sessionReady) return { kind: "remote-unavailable" };
  if (guestLocked) {
    return sessionToken
      ? { kind: "remote", sessionToken, deviceToken }
      : { kind: "remote-unavailable" };
  }
  return { kind: "local", deviceToken };
}

// Confirmed membership loss drops every rail entry of that host: kicked or
// revoked rooms disappear instead of lingering disconnected.
export function lostMemberRoomIds(rooms: RoomDockItem[], serverId: string): string[] {
  return rooms
    .filter((room) => room.roomOrigin === "remote_server" && room.serverId === serverId)
    .map((room) => room.id);
}

// Applies confirmed membership loss (kick/ban/revoke while central is
// connected): the host transport closes, every bearer unbinds, the matching
// session clears, the host's rooms leave the rail, and a stranded selection
// falls back. A temporary central outage never reaches here.
export function applyConfirmedMembershipLost(
  serverId: string,
  state: {
    guestSession: RoomGuestSession | null;
    guestExpired: boolean;
    latestRooms: RoomDockItem[];
    activeRoomId: string;
    memberSelectError: MemberSelectError | null;
  },
  actions: {
    closeHost: (serverId: string) => void;
    unbindHostSessions: (serverId: string) => void;
    clearSession: () => void;
    removeRoom: (roomId: string) => void;
    setSelectError: (error: MemberSelectError | null) => void;
    activateRoom: (roomId: string) => void;
    resetChannel: () => void;
    closeAdmin: () => void;
    clearRetainedRoom: () => void;
  },
): void {
  actions.closeHost(serverId);
  actions.unbindHostSessions(serverId);
  if (state.guestSession && !state.guestExpired && state.guestSession.serverSurface.server_id === serverId) {
    actions.clearSession();
  }
  const lostRoomIds = new Set(lostMemberRoomIds(state.latestRooms, serverId));
  if (lostRoomIds.size === 0) return;
  for (const roomId of lostRoomIds) actions.removeRoom(roomId);
  if (state.memberSelectError && lostRoomIds.has(state.memberSelectError.roomId)) {
    actions.setSelectError(null);
  }
  if (lostRoomIds.has(state.activeRoomId)) {
    const fallback = state.latestRooms.find((room) => !lostRoomIds.has(room.id));
    actions.activateRoom(fallback?.id || "");
    actions.resetChannel();
    actions.closeAdmin();
    actions.clearRetainedRoom();
  }
}
