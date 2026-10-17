import type { CentralDirectoryState } from "../../app/useCentralDirectory";
import type { RoomDockItem } from "../roomDockModel";

/**
 * This computer's server belongs to its registered owner (server_id bound
 * to owner_person_id at registration; central lists it as an owned entry
 * for the signed-in account). When the signed-in account is not its
 * registered owner, the local rooms are not mine and the local operator
 * identity is never presented as the signed-in user.
 */
export function ownedServerIds(directory: CentralDirectoryState | null): string[] {
  if (!directory?.person) return [];
  return (directory.servers || [])
    .filter((server) => server.relation === "owner" && server.server_id)
    .map((server) => server.server_id);
}

export function localRoomServerIds(rooms: RoomDockItem[]): string[] {
  const ids = new Set<string>();
  for (const room of rooms) {
    if (room.roomOrigin === "remote_server") continue;
    if (room.serverId) ids.add(room.serverId);
  }
  return [...ids];
}

/**
 * Whether this computer's local rooms may be shown as the signed-in
 * account's own rooms. Without a signed-in central person (guest/local
 * mode) the local host stays authoritative, preserving existing behaviour.
 * A local room with a known server id is mine only when that id matches one
 * of my owned server ids; rooms without an id (fresh/legacy) keep existing
 * behaviour so a just-created room is never hidden.
 */
export function isLocalOwnedByAccount(
  rooms: RoomDockItem[],
  directory: CentralDirectoryState | null,
): boolean {
  if (!directory?.person) return true;
  const owned = new Set(ownedServerIds(directory));
  if (owned.size === 0) return false;
  const locals = rooms.filter((room) => room.roomOrigin !== "remote_server");
  if (locals.length === 0) return true;
  return locals.some((room) => !room.serverId || owned.has(room.serverId));
}

/**
 * The signed-in account displayed as me when this computer's server is not
 * mine. Never the foreign local operator: the name and avatar come from the
 * central person, so the signed-in account and the displayed self agree.
 */
export function centralAccountSelf(person: { display_name: string; avatar_url?: string | null } | null) {
  if (!person) return undefined;
  return {
    displayName: person.display_name,
    avatarLabel: (person.display_name.slice(0, 1) || "•").toUpperCase(),
    avatarImage: person.avatar_url || undefined,
    statusLabel: "온라인",
    expired: false as const,
  };
}

/**
 * Local rooms visible in my rail. Proven foreign servers (known id, no owned
 * match) never appear as mine; unknown ids and member rooms are preserved.
 */
export function visibleLocalRooms(
  rooms: RoomDockItem[],
  directory: CentralDirectoryState | null,
): RoomDockItem[] {
  if (!directory?.person) return rooms;
  const owned = new Set(ownedServerIds(directory));
  return rooms.filter(
    (room) =>
      room.roomOrigin === "remote_server" ||
      !room.serverId ||
      (owned.size > 0 && owned.has(room.serverId)),
  );
}
