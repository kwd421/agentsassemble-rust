import { useMemo } from "react";
import { loadCentralSession } from "../lib/central/identity";
import { visibleLocalRooms } from "../lib/central/localOwnership";
import { mergeHostRoomsIntoDock, type RoomDockItem } from "../lib/roomDockModel";
import { projectRoomConnections } from "../lib/serverConnectionState";
import { remoteSessionTransport } from "../lib/remote/remoteWorkspace";
import type { CentralOwnerWorkspace } from "../lib/central/ownerWorkspace";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import type { RoomDirectorySyncIssue } from "./useRoomDirectory";
import type { CentralDirectoryState } from "./useCentralDirectory";
import { useMemberHosts } from "./useMemberHosts";

/**
 * Rail room projection from every live host. The owner directory never owns
 * member rooms: each member host's own rooms go disconnected only when that
 * host drops, and the local passthrough keeps this computer's rooms
 * untouched. Auto-attached member hosts own their servers; a guest entry
 * for the same server yields to them.
 */
export function useRailRooms({
  directoryRooms,
  startupHostEnabled,
  roomDirectorySyncIssue,
  centralDirectory,
  ownerConnect,
  admittedSessionToken,
  guestSession,
  guestExpired,
  onConfirmedMembershipLost,
}: {
  directoryRooms: RoomDockItem[];
  startupHostEnabled: boolean;
  roomDirectorySyncIssue: RoomDirectorySyncIssue | null;
  centralDirectory: CentralDirectoryState | null;
  ownerConnect: CentralOwnerWorkspace | null | undefined;
  admittedSessionToken: string;
  guestSession: RoomGuestSession | null;
  guestExpired: boolean;
  onConfirmedMembershipLost?: (serverId: string) => void;
}) {
  const memberHosts = useMemberHosts({
    personId: centralDirectory?.person?.person_id ?? "",
    sessionToken: loadCentralSession()?.token ?? "",
    directory: centralDirectory,
    onConfirmedMembershipLost,
  });
  // This computer's server belongs to its registered owner; proven foreign
  // local rooms never appear as mine (member rooms still come from anchors).
  const visibleDirectoryRooms = useMemo(
    () => visibleLocalRooms(directoryRooms, centralDirectory),
    [directoryRooms, centralDirectory],
  );
  // Every live host besides the directory owner below: their rooms are
  // preserved by the merge while their own session projects their state.
  const remoteHostStates = useMemo(() => {
    const states: Array<{ serverId: string; state: "connected" | "connecting" | "disconnected" | "central-unconfirmed" }> = [];
    const seen = new Set<string>();
    if (ownerConnect) {
      seen.add(ownerConnect.serverId);
      states.push({
        serverId: ownerConnect.serverId,
        state: roomDirectorySyncIssue ? roomDirectorySyncIssue.category === "room_directory_unconfirmed" ? "connecting" : "disconnected" : "connected",
      });
    }
    if (admittedSessionToken && guestSession && !guestExpired && !seen.has(guestSession.serverSurface.server_id)) {
      states.push({
        serverId: guestSession.serverSurface.server_id,
        state: remoteSessionTransport(admittedSessionToken) ? "connected" : "disconnected",
      });
    }
    return states;
  }, [ownerConnect, admittedSessionToken, guestSession, guestExpired, roomDirectorySyncIssue]);
  const rooms = useMemo(() => {
    const memberServerIds = new Set(memberHosts.hostDirs.map((host) => host.serverId));
    const merged = mergeHostRoomsIntoDock(visibleDirectoryRooms, [
      { serverId: "", serverOrigin: "", rooms: null, state: "connected", local: true },
      ...(ownerConnect
        ? [{ serverId: ownerConnect.serverId, serverOrigin: "", rooms: null,
            state: (roomDirectorySyncIssue
              ? roomDirectorySyncIssue.category === "room_directory_unconfirmed" ? "connecting" : "disconnected"
              : "connected") as "connected" | "connecting" | "disconnected" }]
        : []),
      ...(admittedSessionToken && guestSession && !guestExpired && !memberServerIds.has(guestSession.serverSurface.server_id)
        ? [{ serverId: guestSession.serverSurface.server_id, serverOrigin: "", rooms: null,
            state: (remoteSessionTransport(admittedSessionToken) ? "connected" : "disconnected") as "connected" | "disconnected" }]
        : []),
      ...memberHosts.hostDirs,
    ]);
    return projectRoomConnections(merged, startupHostEnabled
      ? roomDirectorySyncIssue ? roomDirectorySyncIssue.category === "room_directory_unconfirmed" ? "connecting" : "disconnected" : "connected"
      : null, centralDirectory?.status === "central-unconfirmed",
      [...remoteHostStates.filter((entry) => !memberServerIds.has(entry.serverId)), ...memberHosts.hostStates]);
  }, [visibleDirectoryRooms, startupHostEnabled, roomDirectorySyncIssue, centralDirectory,
    remoteHostStates, memberHosts.hostDirs, memberHosts.hostStates,
    ownerConnect, admittedSessionToken, guestSession, guestExpired]);
  return { rooms, memberHosts };
}
