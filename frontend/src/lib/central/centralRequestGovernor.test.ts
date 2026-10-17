import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  CENTRAL_REQUESTS_PER_MIN,
  MEMBER_PREVIEW_PER_HOST_PER_10MIN,
  acquireCentralPermit,
  isCentralThrottledError,
  noteCentral429,
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
});
