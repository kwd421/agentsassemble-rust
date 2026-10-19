// Per-viewer convenience: the daily-allowance banner can be closed until the
// next reset. Storage may be unavailable; the banner then simply stays.
const KEY = "aa.quotaNoticeDismissed.v1:";

export function quotaNoticeDismissed(personId: string, resetAtMs: number | null): boolean {
  if (!personId || resetAtMs === null) return false;
  try {
    return window.localStorage.getItem(KEY + personId) === String(resetAtMs);
  } catch {
    return false;
  }
}

export function dismissQuotaNotice(personId: string, resetAtMs: number | null): void {
  if (!personId || resetAtMs === null) return;
  try {
    window.localStorage.setItem(KEY + personId, String(resetAtMs));
  } catch {
    // Not persisted: the banner returns on the next render cycle only.
  }
}
