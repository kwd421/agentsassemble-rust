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
 * Registration-only ownership: this computer's local rooms are mine only
 * when this host's registered server id matches one of the signed-in
 * account's owned server ids (the same authority the old chooser used:
 * owned server id vs `host_device_info` server id). It never depends on
 * whether rooms loaded: while unknown (runtime starting, central
 * unconfirmed, device info unavailable) it is not mine, so the signed-in
 * account is shown instead of a foreign local operator.
 *
 * Without a signed-in central person (guest/local mode) the local host
 * stays authoritative, preserving existing behaviour.
 */
export function isLocalOwnedByRegistration(
  localServerId: string | null | undefined,
  directory: CentralDirectoryState | null,
): boolean {
  if (!directory?.person) return true;
  if (directory.status === "central-unconfirmed") return false;
  if (typeof localServerId !== "string" || !localServerId) return false;
  const owned = new Set(ownedServerIds(directory));
  if (owned.size === 0) return false;
  return owned.has(localServerId);
}

/**
 * Whether this computer's local rooms may be shown as the signed-in
 * account's own rooms. Registration identity owns the answer; the room
 * list is never the authority. Callers pass this host's registered
 * server id (third argument) when available; older two-argument calls
 * keep the previous per-room fallback only for tests.
 */
export function isLocalOwnedByAccount(
  rooms: RoomDockItem[],
  directory: CentralDirectoryState | null,
  localServerId?: string | null | undefined,
): boolean {
  if (arguments.length >= 3) return isLocalOwnedByRegistration(localServerId ?? undefined, directory);
  if (!directory?.person) return true;
  const owned = new Set(ownedServerIds(directory));
  if (owned.size === 0) return false;
  const locals = rooms.filter((room) => room.roomOrigin !== "remote_server");
  if (locals.length === 0) return true;
  return locals.some((room) => !room.serverId || owned.has(room.serverId));
}

/**
 * Quiet mismatch entry: shown only when registration proves a mismatch
 * (known different ids, central confirmed, no conflict, exactly one owned
 * server). While unknown (runtime starting, central unconfirmed, device
 * info unavailable) or without a single owned server, it shows nothing.
 */
export function shouldShowOwnServerMismatch(
  localServerId: string | null | undefined,
  directory: CentralDirectoryState | null,
): boolean {
  if (!directory?.person) return false;
  if (directory.status === "central-unconfirmed") return false;
  if (directory.live?.owner_server_conflict) return false;
  if (typeof localServerId !== "string" || !localServerId) return false;
  const owned = (directory.servers || []).filter((server) => server.relation === "owner");
  if (owned.length !== 1) return false;
  return owned[0].server_id !== localServerId;
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
