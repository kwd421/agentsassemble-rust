import { useCallback, useRef } from "react";
import type { DesktopManagerRoomAuthority } from "../lib/desktopBridge";
import { roomGuestSessionExpired, type RoomGuestSession } from "../lib/roomGuestSession";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { RemoteInviteTransport } from "../api/roomInviteTransport";

/** Transport selection follows workspace entry; metadata never authenticates a request. */
export function useRoomInvitationAccess(
  remoteWorkspace: boolean, rooms: RoomDockItem[], session: RoomGuestSession | null,
  deviceToken: string, localResolve: (roomId: string) => DesktopManagerRoomAuthority,
) {
  const current = useRef({ remoteWorkspace, rooms, session, localResolve });
  current.current = { remoteWorkspace, rooms, session, localResolve };
  const resolve = useCallback((roomId: string): DesktopManagerRoomAuthority => {
    const context = current.current;
    if (!context.remoteWorkspace) return context.localResolve(roomId);
    const matches = context.rooms.filter((room) => room.id === roomId);
    const room = matches.length === 1 ? matches[0] : null;
    const admitted = context.session;
    if (!room || !admitted?.centralOwner || !admitted.sessionToken || !room.roomUid ||
        roomGuestSessionExpired(admitted) ||
        room.meetingId !== admitted.meetingId || room.roomUid !== admitted.roomUid ||
        room.serverId !== admitted.serverSurface.server_id) {
      throw new Error("현재 방의 소유자 접속을 확인할 수 없어요. 방을 다시 열어 주세요.");
    }
    return {
      server_id: admitted.serverSurface.server_id,
      authority_lineage_id: admitted.serverSurface.authority_lineage_id,
      room_id: room.meetingId, room_uid: room.roomUid,
    };
  }, []);
  const remote: RemoteInviteTransport | undefined = remoteWorkspace && session?.centralOwner
    ? { sessionToken: session.sessionToken, deviceToken } : undefined;
  return { resolve, remote };
}
