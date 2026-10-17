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
// own channel close, attach failure, plus explicit retry. No global loop,
// no central polling; grants stay on demand per attempt. A 409
// incarnation_conflict (endpoint revoked / host offline) means the host is
// gone, so it backs off to a slow cadence and wakes on real signals
// (directory refresh on focus/visible/online, republish tuple change, user
// click) instead of timers. A 429 honors the server's Retry-After.
export const MEMBER_HOST_RECONNECT_BASE_MS = 1_000;
export const MEMBER_HOST_RECONNECT_CAP_MS = 30_000;
// Slow cadence while the endpoint stays revoked: 60 s doubling to a 5 min
// cap, so a revoked host costs at most 5 central previews per 10 minutes
// (≈18 per hour) instead of a retry storm.
export const MEMBER_HOST_RECONNECT_SLOW_BASE_MS = 60_000;
export const MEMBER_HOST_RECONNECT_SLOW_CAP_MS = 300_000;
// Default wait after a central 429 without a Retry-After header.
export const MEMBER_HOST_RECONNECT_RATE_LIMIT_MS = 300_000;
export const MEMBER_HOST_RECONNECT_RATE_LIMIT_MAX_MS = 600_000;

export type MemberHostReconnectCause = "offline" | "rate_limited" | "transient";

// 409 incarnation_conflict is the host being offline (central revoked its
// endpoint row or the epoch moved on); 429 is central asking for quiet.
// Both central (plain Error with status/code) and host (ApiError) failures
// carry status/code, so both are classified here.
export function memberHostReconnectCause(error: unknown): MemberHostReconnectCause {
  const status = (error as { status?: unknown } | null)?.status;
  const code = (error as { code?: unknown } | null)?.code;
  if (status === 429) return "rate_limited";
  if (status === 409 && code === "incarnation_conflict") return "offline";
  return "transient";
}

// Milliseconds the server asked to wait on a 429 (parsed from its
// Retry-After header by the central owner), or null when absent.
export function memberHostRetryAfterMs(error: unknown): number | null {
  const wait = (error as { retryAfterMs?: unknown } | null)?.retryAfterMs;
  return typeof wait === "number" && Number.isFinite(wait) && wait >= 0 ? wait : null;
}

function withJitter(delayMs: number, capMs: number, random: () => number): number {
  return Math.min(capMs, Math.floor(delayMs * (0.8 + 0.4 * random())));
}

export function memberHostReconnectDelay(
  attempt: number,
  cause: MemberHostReconnectCause = "transient",
  retryAfterMs: number | null = null,
  random: () => number = Math.random,
): number {
  const step = Number.isSafeInteger(attempt) && attempt > 0 ? attempt : 0;
  if (cause === "rate_limited") {
    // The server dictated this wait: honor it exactly instead of jittering
    // below it. Without a header, rest a long default with jitter.
    if (retryAfterMs !== null) return Math.min(Math.max(Math.ceil(retryAfterMs), 1_000), MEMBER_HOST_RECONNECT_RATE_LIMIT_MAX_MS);
    return withJitter(MEMBER_HOST_RECONNECT_RATE_LIMIT_MS, MEMBER_HOST_RECONNECT_RATE_LIMIT_MAX_MS, random);
  }
  if (cause === "offline") {
    return withJitter(
      MEMBER_HOST_RECONNECT_SLOW_BASE_MS * 2 ** Math.min(step, 3),
      MEMBER_HOST_RECONNECT_SLOW_CAP_MS,
      random,
    );
  }
  return withJitter(
    MEMBER_HOST_RECONNECT_BASE_MS * 2 ** Math.min(step, 5),
    MEMBER_HOST_RECONNECT_CAP_MS,
    random,
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
