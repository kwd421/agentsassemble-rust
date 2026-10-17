// Bounded automatic room re-select budget.
//
// The member auto re-select (foreground open room) and the background live
// subscriptions both re-select on a fresh host admission. A failing select
// internally re-attaches (fresh challenge/grant), which used to look like a
// brand-new host channel and retriggered the automatic select immediately:
// each cycle spent a member-connect grant (3 central units) and, at capacity
// (429 room_admission_capacity_reached) or persistent 401/5xx, burned the
// 240-unit daily allowance in minutes. The per-component `seen` map keyed on
// admission identity could not stop it (new admission every cycle) and reset
// on remount.
//
// This module bounds automatic attempts per host+room independent of
// admission identity and of component remounts (module-level, per account,
// UTC-day window):
// - At most one immediate automatic attempt per fresh EXTERNAL host
//   re-attach. Internal re-attaches performed inside selectRoom (fresh
//   challenge after a failed select, room-list refresh) never bump the host
//   generation, so they never release another immediate attempt.
// - A hard daily cap per host+room for automatic attempts. Manual opens
//   (room tap) and explicit host retry (retryHost / 다시 연결) bypass this
//   budget and still try once immediately.

export const MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP = 5;

export const MEMBER_ROOM_AUTO_RESELECT_STOP_MESSAGE =
  "방을 열지 못했어요. 다시 연결을 눌러 주세요.";

function budgetKey(accountId: string, serverId: string, meetingId: string): string {
  return `${accountId || "anonymous"}\0${serverId}\0${meetingId}`;
}

function hostKey(accountId: string, serverId: string): string {
  return `${accountId || "anonymous"}\0${serverId}`;
}

export function memberRoomAutoReselectDayStartMs(now = Date.now()): number {
  const at = new Date(now);
  return Date.UTC(at.getUTCFullYear(), at.getUTCMonth(), at.getUTCDate());
}

// External host generation: bumped only when the host re-attaches through an
// external signal (reconcile, timer-driven reattach, explicit retry), never by
// the failure path inside selectRoom that re-attaches to retry the same tap.
const hostGenerationByAccountHost = new Map<string, number>();

type RoomBudgetEntry = { dayStartMs: number; count: number; lastGen: number | null };
const roomBudgetByKey = new Map<string, RoomBudgetEntry>();

export function currentMemberHostReselectGeneration(accountId: string, serverId: string): number {
  return hostGenerationByAccountHost.get(hostKey(accountId, serverId)) ?? 0;
}

export function noteMemberHostExternalReselectAttach(accountId: string, serverId: string): number {
  const key = hostKey(accountId, serverId);
  const next = (hostGenerationByAccountHost.get(key) ?? 0) + 1;
  hostGenerationByAccountHost.set(key, next);
  return next;
}

export function memberRoomAutoReselectCount(
  accountId: string,
  serverId: string,
  meetingId: string,
  now = Date.now(),
): number {
  const entry = roomBudgetByKey.get(budgetKey(accountId, serverId, meetingId));
  if (!entry || entry.dayStartMs !== memberRoomAutoReselectDayStartMs(now)) return 0;
  return entry.count;
}

export function memberRoomAutoReselectExhausted(
  accountId: string,
  serverId: string,
  meetingId: string,
  now = Date.now(),
): boolean {
  return (
    memberRoomAutoReselectCount(accountId, serverId, meetingId, now) >=
    MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP
  );
}

export type MemberRoomAutoReselectClaim =
  | { allowed: true }
  | { allowed: false; reason: "already-attempted" | "exhausted" };

/**
 * Claim one automatic attempt for this host+room. At most one claim per
 * external host generation (internal re-attaches share the generation and
 * never release another claim); at most DAILY_CAP claims per UTC day.
 * Denials never increment the count, so a parked room spends no grants.
 */
export function tryClaimMemberRoomAutoReselect(
  accountId: string,
  serverId: string,
  meetingId: string,
  now = Date.now(),
): MemberRoomAutoReselectClaim {
  const dayStartMs = memberRoomAutoReselectDayStartMs(now);
  const key = budgetKey(accountId, serverId, meetingId);
  const gen = currentMemberHostReselectGeneration(accountId, serverId);
  let entry = roomBudgetByKey.get(key);
  if (!entry || entry.dayStartMs !== dayStartMs) {
    entry = { dayStartMs, count: 0, lastGen: null };
    roomBudgetByKey.set(key, entry);
  }
  if (entry.lastGen === gen) return { allowed: false, reason: "already-attempted" };
  if (entry.count >= MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP) {
    return { allowed: false, reason: "exhausted" };
  }
  entry.count += 1;
  entry.lastGen = gen;
  return { allowed: true };
}

/**
 * Claim one extra automatic retry on the SAME host generation (the background
 * dead-session heal: one retry per admission after an authorization_failed
 * socket error). Still bounded by the same daily cap; the per-generation
 * single-claim rule does not apply here because the per-room reauth counter
 * already bounds it to one retry per admission.
 */
export function tryClaimMemberRoomAutoReselectRetry(
  accountId: string,
  serverId: string,
  meetingId: string,
  now = Date.now(),
): MemberRoomAutoReselectClaim {
  const dayStartMs = memberRoomAutoReselectDayStartMs(now);
  const key = budgetKey(accountId, serverId, meetingId);
  let entry = roomBudgetByKey.get(key);
  if (!entry || entry.dayStartMs !== dayStartMs) {
    entry = { dayStartMs, count: 0, lastGen: null };
    roomBudgetByKey.set(key, entry);
  }
  if (entry.count >= MEMBER_ROOM_AUTO_RESELECT_DAILY_CAP) {
    return { allowed: false, reason: "exhausted" };
  }
  entry.count += 1;
  return { allowed: true };
}

export function resetMemberRoomAutoReselectBudgetForTests(): void {
  hostGenerationByAccountHost.clear();
  roomBudgetByKey.clear();
}
