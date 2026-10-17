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

function prune(times: number[], now: number, windowMs: number): number[] {
  while (times.length > 0 && times[0] <= now - windowMs) times.shift();
  return times;
}

export function acquireCentralPermit(path: string, accountId: string): void {
  const account = accountId || "anonymous";
  const now = Date.now();
  const parsed = centralPathFamily(path);
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

export function resetCentralRequestGovernorForTests(): void {
  requestTimesByAccount.clear();
  previewTimesByAccountHost.clear();
  blockedUntilByKey.clear();
  consecutive429ByKey.clear();
}
