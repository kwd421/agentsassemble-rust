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
// Slow cadence while the endpoint stays revoked: 1, 2, 5, 10, 20 min then
// a steady 30–60 min with jitter (uniform in that band). A 5 min cap would
// still spend ≈200 previews/day (≈600 units, far past the 240-unit daily
// allowance where every signed request — even a refused 409 — costs 3
// units); the day-long park below plus the origin probe keep a dead host
// from burning the account. The ladder is per-host attempt count while the
// cause stays offline; any success or explicit user retry restarts it.
export const MEMBER_HOST_RECONNECT_SLOW_BASE_MS = 60_000;
export const MEMBER_HOST_RECONNECT_SLOW_CAP_MS = 3_600_000;
export const MEMBER_HOST_RECONNECT_SLOW_DELAYS_MS = [60_000, 120_000, 300_000, 600_000, 1_200_000];
export const MEMBER_HOST_RECONNECT_SLOW_STEADY_MIN_MS = 1_800_000;
export const MEMBER_HOST_RECONNECT_SLOW_STEADY_MAX_MS = 3_600_000;
// Daily central-reconnect budget while hosts stay unreachable: at most 8
// member-preview requests per joined host per account per UTC day. Each
// signed request costs 3 of the 240-unit daily allowance, so 8 previews =
// 24 units = 10% for one host; the slow ladder above keeps the combined
// spend of a few joined hosts far below the old 5-min-cap storm. Past the
// cap the host parks until the next 00:00 UTC (or an explicit user action:
// 다시 연결 / opening the room).
export const MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP = 8;
// Unsigned origin probe budget before any preview: short enough to keep the
// slow schedule responsive, long enough for a public tunnel origin.
export const MEMBER_HOST_ORIGIN_PROBE_TIMEOUT_MS = 5_000;
// Default wait after a central 429 without a Retry-After header.
export const MEMBER_HOST_RECONNECT_RATE_LIMIT_MS = 300_000;
export const MEMBER_HOST_RECONNECT_RATE_LIMIT_MAX_MS = 600_000;

export type MemberHostReconnectCause = "offline" | "rate_limited" | "transient";

// 409 incarnation_conflict is the host being offline (central revoked its
// endpoint row or the epoch moved on); 429 is central asking for quiet.
// Both central (plain Error with status/code) and host (ApiError) failures
// carry status/code, so both are classified here. The local governor's
// throttled error never touches the network but backs off like a 429.
export function memberHostReconnectCause(error: unknown): MemberHostReconnectCause {
  if ((error as { throttled?: unknown } | null)?.throttled === true) return "rate_limited";
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
    if (step < MEMBER_HOST_RECONNECT_SLOW_DELAYS_MS.length) {
      return withJitter(
        MEMBER_HOST_RECONNECT_SLOW_DELAYS_MS[step],
        MEMBER_HOST_RECONNECT_SLOW_CAP_MS,
        random,
      );
    }
    // Steady state: uniform 30–60 min. random() === 0.5 lands exactly on
    // the 45 min midpoint so fake-timer tests stay deterministic.
    const span = MEMBER_HOST_RECONNECT_SLOW_STEADY_MAX_MS - MEMBER_HOST_RECONNECT_SLOW_STEADY_MIN_MS;
    const picked = MEMBER_HOST_RECONNECT_SLOW_STEADY_MIN_MS + Math.floor(random() * (span + 1));
    return Math.min(Math.max(picked, MEMBER_HOST_RECONNECT_SLOW_STEADY_MIN_MS), MEMBER_HOST_RECONNECT_SLOW_CAP_MS);
  }
  return withJitter(
    MEMBER_HOST_RECONNECT_BASE_MS * 2 ** Math.min(step, 5),
    MEMBER_HOST_RECONNECT_CAP_MS,
    random,
  );
}

// Daily offline-preview budget: at most MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP
// central previews per joined host per account per UTC day. Each signed
// request costs 3 of the 240-unit daily allowance, so 8 previews = 24 units
// = 10% for one host; across a few joined hosts the combined reconnect
// spend stays an order of magnitude below the old 5-min-cap storm (≈200
// previews/day) and leaves the rest for user-initiated joins, reconnects
// and own-server ops. Only automatic retries after an offline (409
// incarnation_conflict) failure spend it; an explicit user action
// (다시 연결 / opening the room) still tries once immediately even when the
// budget is spent. Counts reset at 00:00 UTC.
type OfflineBudgetEntry = { dayStartMs: number; count: number };
const offlinePreviewByAccountHost = new Map<string, OfflineBudgetEntry>();

function offlineBudgetKey(accountId: string, serverId: string): string {
  return `${accountId || "anonymous"}\0${serverId}`;
}

export function memberHostOfflineDayStartMs(now = Date.now()): number {
  const at = new Date(now);
  return Date.UTC(at.getUTCFullYear(), at.getUTCMonth(), at.getUTCDate());
}

export function memberHostOfflinePreviewCount(accountId: string, serverId: string, now = Date.now()): number {
  const entry = offlinePreviewByAccountHost.get(offlineBudgetKey(accountId, serverId));
  if (!entry || entry.dayStartMs !== memberHostOfflineDayStartMs(now)) return 0;
  return entry.count;
}

export function noteMemberHostOfflinePreview(accountId: string, serverId: string, now = Date.now()): number {
  const key = offlineBudgetKey(accountId, serverId);
  const dayStartMs = memberHostOfflineDayStartMs(now);
  const entry = offlinePreviewByAccountHost.get(key);
  const next = entry && entry.dayStartMs === dayStartMs ? entry.count + 1 : 1;
  offlinePreviewByAccountHost.set(key, { dayStartMs, count: next });
  return next;
}

export function memberHostOfflineBudgetExhausted(accountId: string, serverId: string, now = Date.now()): boolean {
  return memberHostOfflinePreviewCount(accountId, serverId, now) >= MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP;
}

export function memberHostOfflineParkMs(now = Date.now()): number {
  return Math.max(0, memberHostOfflineDayStartMs(now) + 24 * 3_600_000 - now);
}

export function resetMemberHostOfflineBudgetForTests(): void {
  offlinePreviewByAccountHost.clear();
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
