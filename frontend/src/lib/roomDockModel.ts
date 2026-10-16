import { sessionHostDisplayOrigin } from "./remote/remoteWorkspace";
import type { ServerConnectionState } from "./serverConnectionState";
import type { LucideIcon } from "lucide-react";
import { Bot, Gamepad2, LayoutDashboard, Radio, Sparkles, Users } from "lucide-react";
import {
  normalizeRoomGlobalSettings,
  type RoomGlobalAppearance,
} from "../api/room";
import type { RoomAppearance } from "./roomAppearance";
import {
  isConnectorInviteToken,
  joinInviteTokenFromUrl,
  loadRoomGuestSession,
  type RoomGuestSession,
} from "./roomGuestSession";
import {
  loadRoomDockItems,
  type PersistedRoomDockItem,
} from "./roomDockPersistence";

export type RoomDockItem = {
  id: string;
  label: string;
  meetingId: string;
  roomUid?: string;
  serverId?: string;
  roomOrigin?: "local" | "remote_server";
  serverOrigin?: string;
  connectionState?: "local" | ServerConnectionState;
  topic: string;
  shortLabel: string;
  appearance?: RoomGlobalAppearance;
  inviteScope?: RoomAppearance["inviteScope"];
  icon: LucideIcon;
  createdAt: string;
  tone: "fresh" | "resident" | "mafia" | "work";
};

export type StartupRoute = {
  guestInvite: RoomDockItem | null;
  guestSession: RoomGuestSession | null;
  guestJoinToken: string;
  connectorJoinUrl: string;
  directRoom: RoomDockItem | null;
  startupRooms: RoomDockItem[];
  activeRoomId: string;
  initialChannel: "lobby";
};

export type ServerRoomDockSource = {
  cleanup_pending?: boolean;
  deletion_pending?: boolean;
  room_id: string;
  room_uid?: string;
  label?: string;
  last_active_at?: string;
  archived?: boolean;
  status?: string;
  room_settings?: unknown;
};

function compactTimestamp(date: Date) {
  const pad = (value: number) => String(value).padStart(2, "0");
  return [
    date.getFullYear(),
    pad(date.getMonth() + 1),
    pad(date.getDate()),
    "T",
    pad(date.getHours()),
    pad(date.getMinutes()),
    pad(date.getSeconds()),
  ].join("");
}

export function createFreshRoom(now = new Date()): RoomDockItem {
  const suffix = compactTimestamp(now);
  return {
    id: `fresh-${suffix}`,
    label: "새 방",
    meetingId: `room-${suffix}`,
    roomOrigin: "local",
    connectionState: "local",
    topic: "빈 채팅방에서 시작",
    shortLabel: "N",
    icon: Sparkles,
    createdAt: now.toISOString(),
    tone: "fresh",
  };
}

function iconForRoomTone(tone: RoomDockItem["tone"]): LucideIcon {
  if (tone === "mafia") return Gamepad2;
  if (tone === "work") return LayoutDashboard;
  if (tone === "resident") return Bot;
  return Sparkles;
}

function sameRoomAppearance(
  left: RoomGlobalAppearance | undefined,
  right: RoomGlobalAppearance | undefined
) {
  return (
    left?.bannerPreset === right?.bannerPreset &&
    left?.bannerImage === right?.bannerImage &&
    left?.iconImage === right?.iconImage &&
    left?.iconLabel === right?.iconLabel &&
    left?.inviteScope === right?.inviteScope
  );
}

export function persistableRoom(room: RoomDockItem): PersistedRoomDockItem {
  return {
    id: room.id,
    label: room.label,
    meetingId: room.meetingId,
    roomUid: room.roomUid,
    serverId: room.serverId,
    roomOrigin: room.roomOrigin === "remote_server" ? "remote_server" : "local",
    serverOrigin: room.roomOrigin === "remote_server" ? room.serverOrigin : undefined,
    topic: room.topic,
    shortLabel: room.shortLabel,
    appearance: room.appearance,
    createdAt: room.createdAt,
    tone: room.tone,
  };
}

export function hydratePersistedRoom(room: PersistedRoomDockItem): RoomDockItem {
  return {
    ...room,
    connectionState: room.roomOrigin === "remote_server" ? "disconnected" : "local",
    icon: iconForRoomTone(room.tone),
  };
}

export function roomIsDisconnected(room: RoomDockItem) {
  return room.connectionState === "disconnected" || room.connectionState === "central-unconfirmed" ||
    room.connectionState === "connecting" || (room.roomOrigin === "remote_server" && room.connectionState !== "connected");
}

export function roomDockIdentity(
  room: Pick<RoomDockItem, "meetingId" | "roomUid" | "serverId" | "roomOrigin" | "serverOrigin">
) {
  if (room.serverId && room.roomUid) {
    return `${room.serverId}:${room.roomUid}`;
  }
  if (room.roomOrigin === "remote_server") {
    return `remote:${room.serverOrigin || "unknown"}:${room.meetingId}`;
  }
  return `local:${room.meetingId}`;
}

export function initialOperatorRooms(directRoom?: RoomDockItem | null) {
  const persisted = loadRoomDockItems()
    .map(hydratePersistedRoom);
  const rooms = persisted;
  if (!directRoom) return rooms;
  const existingIndex = rooms.findIndex(
    (room) => room.id === directRoom.id || roomDockIdentity(room) === roomDockIdentity(directRoom)
  );
  if (existingIndex >= 0) {
    const next = [...rooms];
    next[existingIndex] = {
      ...next[existingIndex],
      label: next[existingIndex].label || directRoom.label,
      topic: next[existingIndex].topic || directRoom.topic,
      shortLabel: next[existingIndex].shortLabel || directRoom.shortLabel,
    };
    return next;
  }
  return [directRoom, ...rooms];
}

export function roomFromServerRoom(
  room: ServerRoomDockSource,
  existing?: Pick<RoomDockItem, "roomOrigin" | "serverOrigin">,
  currentServerOrigin = window.location.origin,
  currentServerId = ""
): RoomDockItem | null {
  const meetingId = String(room.room_id || "");
  const roomUid = String(room.room_uid || "");
  const status = String(room.status || "active").toLowerCase();
  if (!meetingId || room.archived || status === "closed" || status === "archived") return null;
  const settings = normalizeRoomGlobalSettings(room.room_settings, meetingId);
  const label = String(settings?.label || room.label || meetingId).trim() || meetingId;
  const remote =
    existing?.roomOrigin === "remote_server" || currentServerOrigin !== window.location.origin;
  return {
    // Member directory rooms carry no room_uid; scope their dock id by host
    // so two hosts with the same room_id never share one rail entry.
    id: roomUid && currentServerId
      ? `server-${currentServerId}-${roomUid}`
      : remote && currentServerId
        ? `server-${currentServerId}-${meetingId}`
        : `server-${meetingId}`,
    label,
    meetingId,
    roomUid: roomUid || undefined,
    serverId: currentServerId || undefined,
    roomOrigin: remote ? "remote_server" : "local",
    serverOrigin: remote ? (existing?.serverOrigin || currentServerOrigin) : undefined,
    connectionState: remote ? "connected" : "local",
    topic: settings?.topic ?? label,
    shortLabel: settings?.shortLabel || label.slice(0, 1).toUpperCase() || "R",
    appearance: settings?.appearance,
    inviteScope: settings?.appearance.inviteScope,
    icon: Radio,
    createdAt: String(room.last_active_at || ""),
    tone: "resident",
  };
}

export function mergeServerRoomsIntoDock(
  currentRooms: RoomDockItem[],
  serverRooms: ServerRoomDockSource[],
  currentServerOrigin = window.location.origin,
  currentServerId = "",
  options: { local?: boolean; peers?: RoomHostPeer[] } = {}
): RoomDockItem[] {
  const { local = true, peers = [] } = options;
  return mergeHostRoomsIntoDock(currentRooms, [
    { serverId: currentServerId, serverOrigin: currentServerOrigin, rooms: serverRooms, state: "connected", local },
    ...peers.map((peer) => ({
      serverId: peer.serverId ?? "",
      serverOrigin: peer.serverOrigin ?? "",
      rooms: null,
      state: "connected" as ServerConnectionState,
      local: peer.local,
    })),
  ]);
}

export type RoomHostPeer = {
  serverId?: string;
  serverOrigin?: string;
  local?: boolean;
};

export type HostRoomDirectory = {
  serverId: string;
  serverOrigin: string;
  rooms: ServerRoomDockSource[] | null;
  state: ServerConnectionState;
  local?: boolean;
};

function hostForRoom(room: RoomDockItem, hosts: HostRoomDirectory[]): HostRoomDirectory | null {
  if (room.roomOrigin !== "remote_server") {
    return hosts.find((host) => host.local && (!room.serverId || !host.serverId || room.serverId === host.serverId)) ?? null;
  }
  return hosts.find((host) => host.serverId !== "" && room.serverId !== undefined && host.serverId === room.serverId)
    ?? hosts.find((host) => host.serverOrigin !== "" && room.serverOrigin !== undefined && host.serverOrigin === room.serverOrigin)
    ?? null;
}

function indexHostRooms(host: HostRoomDirectory) {
  const byIdentity = new Map<string, ServerRoomDockSource>();
  const byMeetingId = new Map<string, ServerRoomDockSource>();
  for (const room of host.rooms ?? []) {
    const meetingId = String(room.room_id || "");
    const roomUid = String(room.room_uid || "");
    if (!meetingId) continue;
    byMeetingId.set(meetingId, room);
    if (host.serverId && roomUid) {
      byIdentity.set(`${host.serverId}:${roomUid}`, room);
    }
  }
  return { host, byIdentity, byMeetingId };
}

function reconcileRoom(
  room: RoomDockItem,
  serverRoom: ServerRoomDockSource,
  host: HostRoomDirectory
): { room: RoomDockItem; changed: boolean } | null {
  // Rooms owned by a remote host stay remote even when that host shares the
  // shell origin; only the local host's own rooms compare against it.
  const existing = host.local
    ? room
    : { ...room, roomOrigin: "remote_server" as const, serverOrigin: room.serverOrigin ?? host.serverOrigin };
  const canonicalRoom = roomFromServerRoom(serverRoom, existing, host.serverOrigin, host.serverId);
  if (!canonicalRoom) return null;
  const reconciledRoom = {
    ...room,
    label: canonicalRoom.label,
    meetingId: canonicalRoom.meetingId,
    topic: canonicalRoom.topic,
    shortLabel: canonicalRoom.shortLabel,
    appearance: canonicalRoom.appearance,
    inviteScope: canonicalRoom.inviteScope,
    icon: canonicalRoom.icon,
    createdAt: canonicalRoom.createdAt,
    tone: canonicalRoom.tone,
    roomOrigin: canonicalRoom.roomOrigin,
    roomUid: canonicalRoom.roomUid,
    serverId: canonicalRoom.serverId,
    serverOrigin: canonicalRoom.serverOrigin,
    connectionState: canonicalRoom.connectionState,
  };
  const changed =
    room.label !== reconciledRoom.label ||
    room.meetingId !== reconciledRoom.meetingId ||
    room.topic !== reconciledRoom.topic ||
    room.shortLabel !== reconciledRoom.shortLabel ||
    !sameRoomAppearance(room.appearance, reconciledRoom.appearance) ||
    room.inviteScope !== reconciledRoom.inviteScope ||
    room.icon !== reconciledRoom.icon ||
    room.createdAt !== reconciledRoom.createdAt ||
    room.tone !== reconciledRoom.tone ||
    room.roomOrigin !== reconciledRoom.roomOrigin ||
    room.roomUid !== reconciledRoom.roomUid ||
    room.serverId !== reconciledRoom.serverId ||
    room.serverOrigin !== reconciledRoom.serverOrigin ||
    room.connectionState !== reconciledRoom.connectionState;
  return { room: changed ? reconciledRoom : room, changed };
}

function disconnectRoom(room: RoomDockItem, state: ServerConnectionState): { room: RoomDockItem; changed: boolean } {
  if (room.connectionState === state) return { room, changed: false };
  return { room: { ...room, connectionState: state }, changed: true };
}

export function mergeHostRoomsIntoDock(
  currentRooms: RoomDockItem[],
  hosts: HostRoomDirectory[]
): RoomDockItem[] {
  const indexed = hosts.map(indexHostRooms);
  const next: RoomDockItem[] = [];
  let changed = false;

  for (const room of currentRooms) {
    const owner = hostForRoom(room, hosts);
    const index = owner ? indexed[hosts.indexOf(owner)] : null;
    if (!owner || !index) {
      // No live host owns this room: it stays visible but disconnected, and
      // is never silently dropped or revived by another host's directory.
      const resolved = disconnectRoom(room, "disconnected");
      if (resolved.changed) changed = true;
      next.push(resolved.room);
      continue;
    }
    if (!owner.rooms || owner.state !== "connected") {
      if (!owner.rooms && owner.state === "connected") {
        next.push(room);
        continue;
      }
      const resolved = disconnectRoom(room, owner.state);
      if (resolved.changed) changed = true;
      next.push(resolved.room);
      continue;
    }
    const serverRoom = index.byIdentity.get(roomDockIdentity(room))
      || index.byMeetingId.get(room.meetingId);
    const resolved = serverRoom ? reconcileRoom(room, serverRoom, owner) : null;
    if (!resolved) {
      // The owning host is live but no longer lists this room: the friend
      // removed, kicked or revoked it, so it disappears without a tombstone.
      changed = true;
      continue;
    }
    if (resolved.changed) changed = true;
    next.push(resolved.room);
  }

  const seenRoomIdentities = new Set(next.map(roomDockIdentity));
  for (const { host } of indexed) {
    if (!host.rooms) continue;
    for (const serverRoom of host.rooms) {
      const dockRoom = roomFromServerRoom(
        serverRoom,
        host.local ? undefined : { roomOrigin: "remote_server", serverOrigin: host.serverOrigin },
        host.serverOrigin,
        host.serverId
      );
      if (!dockRoom || seenRoomIdentities.has(roomDockIdentity(dockRoom))) continue;
      // A known host that is down still contributes its last known rooms,
      // shown disconnected; unknown/new rooms are never invented.
      next.push(host.state === "connected" ? dockRoom : { ...dockRoom, connectionState: host.state });
      seenRoomIdentities.add(roomDockIdentity(dockRoom));
      changed = true;
    }
  }
  return changed ? next : currentRooms;
}

function cleanInviteValue(value: string | null, fallback: string, limit: number) {
  const text = (value || "").replace(/[\r\n\t]/g, " ").trim();
  return (text || fallback).slice(0, limit);
}

export function roomFromGuestSession(session: RoomGuestSession): RoomDockItem {
  const label = session.roomLabel || session.meetingId || "초대받은 방";
  return {
    id:
      session.roomUid
        ? `server-${session.serverSurface.server_id}-${session.roomUid}`
        : `guest-session-${session.meetingId || session.agentId}`,
    label,
    meetingId: session.meetingId,
    roomUid: session.roomUid,
    serverId: session.serverSurface.server_id,
    roomOrigin: "remote_server",
    // Display only; routing always uses serverId/bearer. Unknown until the
    // host's session is bound, and never another host's origin.
    serverOrigin: sessionHostDisplayOrigin(session.sessionToken) ?? undefined,
    connectionState: "connected",
    topic: session.roomTopic || `${session.displayName || session.agentId}로 입장한 방`,
    shortLabel: label.slice(0, 1).toUpperCase() || "G",
    inviteScope: session.inviteScope,
    icon: Users,
    createdAt: session.roomCreatedAt || "",
    tone: "resident",
  };
}

function roomFromPendingAdmission(kind: "invite" | "pairing"): RoomDockItem {
  const pairing = kind === "pairing";
  return {
    id: pairing ? "operator-pairing-pending" : "guest-join-pending",
    label: pairing ? "운영자 기기 연결 중" : "초대 확인 중",
    meetingId: pairing ? "pending-pairing" : "pending-join",
    topic: pairing ? "공개 주소의 브라우저 신원을 연결하는 중" : "초대 링크로 방에 입장하는 중",
    shortLabel: "G",
    inviteScope: "room",
    icon: Users,
    createdAt: "",
    tone: "resident",
  };
}

export function roomFromDirectParams(): RoomDockItem | null {
  try {
    const query = new URLSearchParams(window.location.search);
    const guestMode =
      query.get("guest") === "1" ||
      query.get("invite") === "1" ||
      query.get("invite") === "room";
    const meetingId = cleanInviteValue(
      query.get("room") || query.get("meeting") || query.get("meeting_id"),
      "",
      128
    );
    if (guestMode || !meetingId) return null;
    const label = cleanInviteValue(query.get("roomName") || query.get("name"), meetingId, 80);
    const topic = cleanInviteValue(query.get("topic"), "직접 열린 방", 160);
    return {
      id: `direct-${meetingId}`,
      label: label || meetingId,
      meetingId,
      topic,
      shortLabel: (label || meetingId).slice(0, 1).toUpperCase() || "R",
      icon: Users,
      createdAt: "",
      tone: "resident",
    };
  } catch {
    return null;
  }
}

export function roomFromMafiaParams(): RoomDockItem | null {
  try {
    const query = new URLSearchParams(window.location.search);
    const gameId = cleanInviteValue(query.get("mafia") || query.get("mafiaGameId"), "", 128);
    if (!gameId) return null;
    const label = cleanInviteValue(query.get("roomName") || query.get("name"), "Mafia Night", 80);
    const topic = cleanInviteValue(query.get("topic"), "Play Mode 마피아", 160);
    return {
      id: `mafia-${gameId}`,
      label,
      meetingId: gameId,
      topic,
      shortLabel: "M",
      icon: Gamepad2,
      createdAt: "",
      tone: "mafia",
    };
  } catch {
    return null;
  }
}

function activeRoomIdForStartup(rooms: RoomDockItem[], routeRoom?: RoomDockItem | null) {
  if (!routeRoom) return "";
  return (
    rooms.find(
      (room) => room.id === routeRoom.id || roomDockIdentity(room) === roomDockIdentity(routeRoom)
    )?.id ||
    routeRoom.id
  );
}

export function createStartupRoute({ operatorPairingPending = false, memberInviteToken = "", memberConnect = false } = {}): StartupRoute {
  const rawJoinToken = operatorPairingPending ? "" : memberInviteToken || joinInviteTokenFromUrl(window.location.href);
  const connectorJoinUrl = isConnectorInviteToken(rawJoinToken)
    ? `${window.location.origin}/join?token=${rawJoinToken}`
    : "";
  const guestJoinToken = connectorJoinUrl ? "" : rawJoinToken;
  const guestSession = loadRoomGuestSession();
  const guestInvite =
    operatorPairingPending
      ? roomFromPendingAdmission("pairing")
      : guestJoinToken || connectorJoinUrl || memberConnect
      ? roomFromPendingAdmission("invite")
      : guestSession
        ? roomFromGuestSession(guestSession)
        : null;
  const directRoom = guestInvite ? null : roomFromDirectParams();
  const startupRooms = guestInvite ? [guestInvite] : initialOperatorRooms(directRoom);
  return {
    guestInvite,
    guestSession,
    guestJoinToken,
    connectorJoinUrl,
    directRoom,
    startupRooms,
    activeRoomId: guestInvite?.id || activeRoomIdForStartup(startupRooms, directRoom),
    initialChannel: "lobby",
  };
}

export function roomFromFlow(flow: { meeting_id?: string; topic?: string }): RoomDockItem | null {
  if (!flow.meeting_id) return null;
  return {
    id: `flow-${flow.meeting_id}`,
    label: flow.meeting_id,
    meetingId: flow.meeting_id,
    topic: flow.topic || "최근 회의",
    shortLabel: "R",
    icon: Radio,
    createdAt: "",
    tone: "resident",
  };
}

export function roomHasAgent(room: RoomDockItem, agent: { meeting_id?: string }) {
  return Boolean(agent.meeting_id && agent.meeting_id === room.meetingId);
}

export function roomSettingsKey(room: RoomDockItem) {
  return room.meetingId ? roomDockIdentity(room) : room.id;
}
