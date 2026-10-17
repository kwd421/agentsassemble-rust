import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CENTRAL_REQUESTS_PER_MIN,
  MEMBER_PREVIEW_PER_HOST_PER_10MIN,
  acquireCentralPermit,
  centralDailyQuotaActionNotice,
  centralDailyQuotaNotice,
  centralDailyQuotaResetAtMs,
  centralDailyQuotaWaitMs,
  centralThrottledWaitMs,
  isCentralThrottledError,
  nextCentralQuotaResetMs,
  noteCentral429,
  noteCentralDailyQuotaExhausted,
  noteCentralSuccess,
  resetCentralRequestGovernorForTests,
} from "./centralRequestGovernor";

describe("centralRequestGovernor", () => {
  beforeEach(() => {
    resetCentralRequestGovernorForTests();
    vi.useRealTimers();
  });

  it("caps member-preview per host per 10 minutes", () => {
    vi.useFakeTimers();
    try {
      const path = "/v1/servers/friend/member-preview";
      for (let i = 0; i < MEMBER_PREVIEW_PER_HOST_PER_10MIN; i += 1) {
        acquireCentralPermit(path, "person");
        // Spread permits so the per-minute total cap does not interfere.
        vi.advanceTimersByTime(61_000);
      }
      expect(() => acquireCentralPermit(path, "person")).toThrowError();
      try {
        acquireCentralPermit(path, "person");
        expect.unreachable();
      } catch (error) {
        expect(isCentralThrottledError(error)).toBe(true);
      }
      // After the 10-minute window the host may preview again.
      vi.advanceTimersByTime(10 * 60_000);
      acquireCentralPermit(path, "person");
    } finally {
      vi.useRealTimers();
    }
  });

  it("caps total central requests per minute per account", () => {
    vi.useFakeTimers();
    try {
      for (let i = 0; i < CENTRAL_REQUESTS_PER_MIN; i += 1) {
        acquireCentralPermit(`/v1/other/${i}`, "person");
      }
      expect(() => acquireCentralPermit("/v1/bootstrap", "person")).toThrowError();
      vi.advanceTimersByTime(60_000);
      acquireCentralPermit("/v1/bootstrap", "person");
    } finally {
      vi.useRealTimers();
    }
  });

  it("honors Retry-After and backs off further on repeated 429", () => {
    vi.useFakeTimers();
    try {
      const path = "/v1/servers/friend/member-preview";
      acquireCentralPermit(path, "person");
      noteCentral429(path, "person", 120_000);
      expect(() => acquireCentralPermit(path, "person")).toThrowError();
      vi.advanceTimersByTime(119_000);
      expect(() => acquireCentralPermit(path, "person")).toThrowError();
      vi.advanceTimersByTime(2_000);
      acquireCentralPermit(path, "person");
      // A second 429 without a header rests longer than the first default.
      noteCentral429(path, "person", null);
      const firstBlock = (() => {
        try {
          acquireCentralPermit(path, "person");
          return 0;
        } catch (error) {
          return (error as { retryAfterMs: number }).retryAfterMs;
        }
      })();
      expect(firstBlock).toBeGreaterThanOrEqual(300_000);
    } finally {
      vi.useRealTimers();
    }
  });

  it("isolates accounts and resets consecutive 429s on success", () => {
    vi.useFakeTimers();
    try {
      const path = "/v1/servers/friend/member-preview";
      acquireCentralPermit(path, "alice");
      noteCentral429(path, "alice", 60_000);
      // Another account is unaffected by alice's backoff.
      acquireCentralPermit(path, "bob");
      noteCentralSuccess(path, "alice");
      vi.advanceTimersByTime(61_000);
      acquireCentralPermit(path, "alice");
    } finally {
      vi.useRealTimers();
    }
  });

  it("parks every path family until UTC midnight on daily quota exhaustion", () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date(Date.UTC(2026, 9, 17, 1, 0, 0)));
      expect(nextCentralQuotaResetMs()).toBe(Date.UTC(2026, 9, 18));
      noteCentralDailyQuotaExhausted("person", Date.now(), 5_000);
      // Every path family is blocked for this account ...
      for (const path of [
        "/v1/bootstrap",
        "/v1/servers",
        "/v1/servers/friend/member-preview",
        "/v1/servers/friend/member-connect-grants",
      ]) {
        expect(() => acquireCentralPermit(path, "person")).toThrowError();
      }
      expect(centralThrottledWaitMs("/v1/bootstrap", "person")).toBeGreaterThan(20 * 3_600_000);
      // ... but another account is unaffected.
      acquireCentralPermit("/v1/bootstrap", "bob");
      expect(centralDailyQuotaResetAtMs("person")).toBe(Date.UTC(2026, 9, 18));
      expect(centralDailyQuotaNotice("person")).toContain("이후에 다시 연결돼요.");
      expect(centralDailyQuotaNotice("bob")).toBeNull();
      // Explicit actions get the neutral line with the local reset time,
      // never the friend-room reconnect line.
      expect(centralDailyQuotaActionNotice("person")).toContain("이후에 다시 해 주세요.");
      expect(centralDailyQuotaActionNotice("person")).not.toContain("친구 방");
      expect(centralDailyQuotaActionNotice("bob")).toBeNull();
      // Just before the reset the block still holds; past reset+jitter it lifts.
      vi.setSystemTime(new Date(Date.UTC(2026, 9, 18) - 1_000));
      expect(() => acquireCentralPermit("/v1/bootstrap", "person")).toThrowError();
      vi.setSystemTime(new Date(Date.UTC(2026, 9, 18) + 5_000));
      acquireCentralPermit("/v1/bootstrap", "person");
      expect(centralDailyQuotaResetAtMs("person")).toBeNull();
      expect(centralDailyQuotaNotice("person")).toBeNull();
      expect(centralDailyQuotaActionNotice("person")).toBeNull();
      expect(centralDailyQuotaWaitMs("person")).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("leaves ordinary 429 handling quota-free", () => {
    vi.useFakeTimers();
    try {
      acquireCentralPermit("/v1/bootstrap", "person");
      noteCentral429("/v1/bootstrap", "person", 60_000);
      expect(centralDailyQuotaWaitMs("person")).toBe(0);
      expect(centralDailyQuotaResetAtMs("person")).toBeNull();
      expect(centralDailyQuotaNotice("person")).toBeNull();
      expect(centralDailyQuotaActionNotice("person")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});
