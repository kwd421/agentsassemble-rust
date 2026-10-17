// Single hard governor for every central fetch. No caller bypasses it:
// `signedFetch` and `unsignedPost` in `identity.ts` are the only owners that
// touch the central origin, and both acquire a permit here first.

export const MEMBER_PREVIEW_PER_HOST_PER_10MIN = 6;
export const MEMBER_PREVIEW_WINDOW_MS = 10 * 60_000;
export const CENTRAL_REQUESTS_PER_MIN = 20;
export const CENTRAL_WINDOW_MS = 60_000;
const RATE_LIMIT_DEFAULT_MS = 300_000;
const RATE_LIMIT_MAX_MS = 600_000;

import { CentralTemporaryError } from "./connectionError";

export class CentralThrottledError extends CentralTemporaryError {
  readonly throttled = true as const;
  readonly status = 429 as const;
  readonly code = "central_throttled";
  constructor(message: string, readonly retryAfterMs: number, readonly family: string) {
    super(message);
    this.name = "CentralThrottledError";
  }
}

export function isCentralThrottledError(error: unknown): boolean {
  return error instanceof CentralThrottledError;
}

export type CentralPathFamily = {
  family: "member-preview" | "member-grant" | "bootstrap" | "other";
  hostId: string | null;
};

const PREVIEW_RE = /\/v1\/servers\/([^/]+)\/member-preview/;
const GRANT_RE = /\/v1\/servers\/([^/]+)\/(member-connect-grants|member-grants|connect-grants)/;

export function centralPathFamily(path: string): CentralPathFamily {
  const preview = PREVIEW_RE.exec(path);
  if (preview) return { family: "member-preview", hostId: preview[1] };
  const grant = GRANT_RE.exec(path);
  if (grant) return { family: "member-grant", hostId: grant[1] };
  if (path === "/v1/bootstrap" || path.startsWith("/v1/bootstrap?")) return { family: "bootstrap", hostId: null };
  return { family: "other", hostId: null };
}

function familyKey(accountId: string, parsed: CentralPathFamily): string {
  return parsed.hostId ? `${accountId}\0${parsed.family}\0${parsed.hostId}` : `${accountId}\0${parsed.family}`;
}

const requestTimesByAccount = new Map<string, number[]>();
const previewTimesByAccountHost = new Map<string, number[]>();
const blockedUntilByKey = new Map<string, number>();
const consecutive429ByKey = new Map<string, number>();

// Daily signed-request allowance: central answers 429
// `actor_quota_exhausted` (never with Retry-After) when the account's or
// session's daily allowance is spent. Counters reset at the next 00:00 UTC
// and a rejected request spends nothing, so the account parks every path
// family until then instead of polling like a short rate limit.
const quotaBlockedUntilByAccount = new Map<string, number>();
const quotaResetAtByAccount = new Map<string, number>();
const QUOTA_JITTER_MAX_MS = 60_000;

export function nextCentralQuotaResetMs(now = Date.now()): number {
  const at = new Date(now);
  return Date.UTC(at.getUTCFullYear(), at.getUTCMonth(), at.getUTCDate() + 1);
}

// Records a spent daily allowance for the account. Returns the remaining
// block in milliseconds. The block ends at the next 00:00 UTC plus a small
// jitter so many clients do not retry in the same second.
export function noteCentralDailyQuotaExhausted(accountId: string, now = Date.now(), jitterMs?: number): number {
  const account = accountId || "anonymous";
  const resetAt = nextCentralQuotaResetMs(now);
  const jitter = jitterMs === undefined
    ? Math.floor(Math.random() * (QUOTA_JITTER_MAX_MS + 1))
    : Math.min(Math.max(Math.ceil(jitterMs), 0), QUOTA_JITTER_MAX_MS);
  const blockedUntil = resetAt + jitter;
  if (blockedUntil > (quotaBlockedUntilByAccount.get(account) ?? 0)) {
    quotaBlockedUntilByAccount.set(account, blockedUntil);
    quotaResetAtByAccount.set(account, resetAt);
  }
  return Math.max(0, (quotaBlockedUntilByAccount.get(account) ?? blockedUntil) - now);
}

// Milliseconds until the account's daily quota block lifts, or zero when
// no block is in force. Expired blocks are dropped so the reset time reads
// empty afterwards.
export function centralDailyQuotaWaitMs(accountId: string, now = Date.now()): number {
  const account = accountId || "anonymous";
  const blocked = quotaBlockedUntilByAccount.get(account) ?? 0;
  if (now >= blocked) {
    quotaBlockedUntilByAccount.delete(account);
    quotaResetAtByAccount.delete(account);
    return 0;
  }
  return blocked - now;
}

// The 00:00 UTC reset this block waits for, or null when unblocked.
export function centralDailyQuotaResetAtMs(accountId: string, now = Date.now()): number | null {
  if (centralDailyQuotaWaitMs(accountId, now) <= 0) return null;
  return quotaResetAtByAccount.get(accountId || "anonymous") ?? null;
}

export function formatCentralQuotaResetLocal(resetAtMs: number): string {
  return new Intl.DateTimeFormat("ko-KR", { hour: "numeric", minute: "2-digit", hour12: true }).format(new Date(resetAtMs));
}

// One Korean line shown while the daily block is active, or null when
// unblocked. The single copy owner for the quota notice.
export function centralDailyQuotaNotice(accountId: string, now = Date.now()): string | null {
  const resetAt = centralDailyQuotaResetAtMs(accountId, now);
  if (resetAt === null) return null;
  return `오늘 연결 한도를 다 써서 친구 방은 ${formatCentralQuotaResetLocal(resetAt)} 이후에 다시 연결돼요.`;
}

// One neutral Korean line for an explicit user action (rename, icon, …)
// refused by the daily block, or null when unblocked. The friend-room
// reconnect line stays on the reconnect/notice surfaces only; an explicit
// action names no room and carries the local reset time instead.
export function centralDailyQuotaActionNotice(accountId: string, now = Date.now()): string | null {
  const resetAt = centralDailyQuotaResetAtMs(accountId, now);
  if (resetAt === null) return null;
  return `오늘 요청 한도를 다 썼어요. ${formatCentralQuotaResetLocal(resetAt)} 이후에 다시 해 주세요.`;
}

function prune(times: number[], now: number, windowMs: number): number[] {
  while (times.length > 0 && times[0] <= now - windowMs) times.shift();
  return times;
}

export function acquireCentralPermit(path: string, accountId: string): void {
  const account = accountId || "anonymous";
  const now = Date.now();
  const parsed = centralPathFamily(path);
  const quotaWait = centralDailyQuotaWaitMs(account, now);
  if (quotaWait > 0) {
    throw new CentralThrottledError(
      "중앙 요청이 잠시 멈췄어요. 잠시 후 다시 시도해 주세요.",
      quotaWait,
      parsed.family,
    );
  }
  const key = familyKey(account, parsed);
  const globalBlockKey = `${account}\0global`;
  const blocked = blockedUntilByKey.get(key) ?? 0;
  if (now < blocked) {
    throw new CentralThrottledError(
      "중앙 요청이 잠시 멈췄어요. 잠시 후 다시 시도해 주세요.",
      blocked - now,
      parsed.family,
    );
  }
  const globalBlocked = blockedUntilByKey.get(globalBlockKey) ?? 0;
  if (now < globalBlocked) {
    throw new CentralThrottledError(
      "중앙 요청이 잠시 멈췄어요. 잠시 후 다시 시도해 주세요.",
      globalBlocked - now,
      parsed.family,
    );
  }
  if (parsed.family === "member-preview" && parsed.hostId) {
    const previewKey = `${account}\0${parsed.hostId}`;
    const times = prune(previewTimesByAccountHost.get(previewKey) ?? [], now, MEMBER_PREVIEW_WINDOW_MS);
    previewTimesByAccountHost.set(previewKey, times);
    if (times.length >= MEMBER_PREVIEW_PER_HOST_PER_10MIN) {
      throw new CentralThrottledError(
        "중앙 요청이 잠시 멈췄어요. 잠시 후 다시 시도해 주세요.",
        Math.max(1_000, times[0] + MEMBER_PREVIEW_WINDOW_MS - now),
        parsed.family,
      );
    }
  }
  const globalTimes = prune(requestTimesByAccount.get(account) ?? [], now, CENTRAL_WINDOW_MS);
  requestTimesByAccount.set(account, globalTimes);
  if (globalTimes.length >= CENTRAL_REQUESTS_PER_MIN) {
    throw new CentralThrottledError(
      "중앙 요청이 잠시 멈췄어요. 잠시 후 다시 시도해 주세요.",
      Math.max(1_000, globalTimes[0] + CENTRAL_WINDOW_MS - now),
      parsed.family,
    );
  }
  globalTimes.push(now);
  if (parsed.family === "member-preview" && parsed.hostId) {
    previewTimesByAccountHost.get(`${account}\0${parsed.hostId}`)?.push(now);
  }
}

export function noteCentral429(path: string, accountId: string, retryAfterMs: number | null): void {
  const account = accountId || "anonymous";
  const now = Date.now();
  const parsed = centralPathFamily(path);
  const key = familyKey(account, parsed);
  const consecutive = (consecutive429ByKey.get(key) ?? 0) + 1;
  consecutive429ByKey.set(key, consecutive);
  const backoff = Math.min(RATE_LIMIT_DEFAULT_MS * 2 ** Math.min(consecutive - 1, 1), RATE_LIMIT_MAX_MS);
  const wait = retryAfterMs !== null && Number.isFinite(retryAfterMs) && retryAfterMs >= 0
    ? Math.min(Math.max(Math.ceil(retryAfterMs), 1_000), RATE_LIMIT_MAX_MS)
    : backoff;
  // Repeated 429s back off further even when the server repeats the header.
  const grown = consecutive > 1 ? Math.min(Math.max(wait, backoff), RATE_LIMIT_MAX_MS) : wait;
  blockedUntilByKey.set(key, now + grown);
  const globalBlockKey = `${account}\0global`;
  blockedUntilByKey.set(globalBlockKey, Math.max(blockedUntilByKey.get(globalBlockKey) ?? 0, now + Math.min(grown, RATE_LIMIT_MAX_MS)));
}

export function noteCentralSuccess(path: string, accountId: string): void {
  const account = accountId || "anonymous";
  consecutive429ByKey.delete(familyKey(account, centralPathFamily(path)));
}

// Milliseconds until this path may be tried again under the governor:
// the longest of the path-family block, the account global block and the
// daily quota block, or zero when no block is in force. Lets bounded retry
// schedules honor the governor instead of firing into a local throttle
// (budgets unchanged).
export function centralThrottledWaitMs(path: string, accountId: string): number {
  const account = accountId || "anonymous";
  const now = Date.now();
  const parsed = centralPathFamily(path);
  const familyBlocked = blockedUntilByKey.get(familyKey(account, parsed)) ?? 0;
  const globalBlocked = blockedUntilByKey.get(`${account}\0global`) ?? 0;
  return Math.max(0, familyBlocked - now, globalBlocked - now, centralDailyQuotaWaitMs(account, now));
}

export function resetCentralRequestGovernorForTests(): void {
  requestTimesByAccount.clear();
  previewTimesByAccountHost.clear();
  blockedUntilByKey.clear();
  consecutive429ByKey.clear();
  quotaBlockedUntilByAccount.clear();
  quotaResetAtByAccount.clear();
}
