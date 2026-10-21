import { createContext, useContext } from "react";

/**
 * Which server's owner routes (memory, AI handoff trust) an agent card talks
 * to: null for this computer's own server, a server id for a remote room the
 * viewer owns, undefined when the room's server is unknown (owner-only
 * controls stay hidden). Never "whichever owner workspace is connected".
 */
export const OwnerRouteHost = createContext<string | null | undefined>(null);

export function ownerRouteHostFor(room: { roomOrigin?: string; serverId?: string }): string | null | undefined {
  if (room.roomOrigin !== "remote_server") return null;
  return room.serverId || undefined;
}

export function useOwnerRouteHost(): string | null | undefined {
  return useContext(OwnerRouteHost);
}

/** The other people in the active room, for choices an agent card offers about them. */
export type RoomPerson = { id: string; name: string };
export const RoomPeople = createContext<RoomPerson[]>([]);

export function useRoomPeople(): RoomPerson[] {
  return useContext(RoomPeople);
}
