import type { RoomEvent } from "../../api";

// Live rail state for one joined member room: only what the rail icon
// needs (unread count source, mention flag, connection). The full chat
// view stays closed; the canonical socket owns the active room.
export type MemberRoomLiveBadge = {
  unread: boolean;
  mention: boolean;
};

// Exactly one secure channel per host, never per room. Background room
// sockets multiplex over it (RemoteTransport.openSocket, capped at 32
// streams per channel). Eight background rooms leave headroom for the
// active room socket, sidechat, history and HTTP on the same channel;
// rooms past the cap stay listed with last-known state and no live badge
// (unknown shown as unknown, never fabricated).
export const MEMBER_ROOM_LIVE_SOCKETS_PER_HOST = 8;

// Per-host reconnect backoff: bounded doubling driven only by that host's
// own channel close plus explicit retry. No global loop, no central
// polling; grants stay on demand per attempt.
export const MEMBER_HOST_RECONNECT_BASE_MS = 1_000;
export const MEMBER_HOST_RECONNECT_CAP_MS = 30_000;

export function memberHostReconnectDelay(attempt: number): number {
  const step = Number.isSafeInteger(attempt) && attempt > 0 ? attempt : 0;
  return Math.min(
    MEMBER_HOST_RECONNECT_CAP_MS,
    MEMBER_HOST_RECONNECT_BASE_MS * 2 ** Math.min(step, 5)
  );
}

export type MemberRoomViewer = {
  participantId: string;
  displayName: string;
};

// Same owner as LobbyView: the viewer's own messages (from any device)
// are never unread, and lobby system lines are history, not unread.
// Custom channel records are always real messages.
function isLiveRealMessage(event: RoomEvent, viewerParticipantId: string): boolean {
  if (event.participant_id === viewerParticipantId) return false;
  if (event.type === "message_final") return event.message_kind !== "system";
  return event.type === "channel_message_final";
}

// Composer writes structured `<@id>` tokens (discordTextTokens) and plain
// `@name` mentions; both count for the rail mention flag.
export function messageMentionsViewer(
  content: unknown,
  viewer: MemberRoomViewer
): boolean {
  if (typeof content !== "string" || !content) return false;
  if (viewer.participantId && content.includes(`<@${viewer.participantId}>`)) return true;
  const name = viewer.displayName.trim();
  return Boolean(name) && content.includes(`@${name}`);
}

export function projectMemberRoomBadge({
  events,
  floorSeq,
  viewer,
}: {
  events: readonly RoomEvent[];
  floorSeq: number;
  viewer: MemberRoomViewer;
}): { badge: MemberRoomLiveBadge; latestSeq: number } {
  const badge: MemberRoomLiveBadge = { unread: false, mention: false };
  let latestSeq = floorSeq;
  for (const event of events) {
    const seq = Number(event.seq) || 0;
    if (seq > latestSeq) latestSeq = seq;
    if (seq <= floorSeq || !isLiveRealMessage(event, viewer.participantId)) continue;
    badge.unread = true;
    if (messageMentionsViewer(event.content, viewer)) badge.mention = true;
  }
  return { badge, latestSeq };
}

// Background coverage: the active room is owned by the canonical socket
// and never counted; the rest fill the per-host cap in recent-activity
// order (most recent first).
export function pickLiveRoomIds(
  roomIds: readonly string[],
  activeMeetingId: string,
  cap: number = MEMBER_ROOM_LIVE_SOCKETS_PER_HOST
): string[] {
  return roomIds.filter((roomId) => roomId !== activeMeetingId).slice(0, Math.max(0, cap));
}

// Rail badge key: one live subscription per host room.
export function memberRoomLiveKey(serverId: string, meetingId: string): string {
  return `${serverId}\0${meetingId}`;
}
