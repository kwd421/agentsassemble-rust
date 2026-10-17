import { act, cleanup, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCentralDirectory } from "./useCentralDirectory";
import { useMemberHosts } from "./useMemberHosts";
import { useMemberRoomLive } from "./useMemberRoomLive";
import { acquireCentralPermit, resetCentralRequestGovernorForTests } from "../lib/central/centralRequestGovernor";

const bootstrap = vi.fn();
const attach = vi.fn();

vi.mock("../lib/central/identity", async (original) => {
  const actual = await original<typeof import("../lib/central/identity")>();
  return {
    ...actual,
    bootstrapCentral: (...args: unknown[]) => bootstrap(...args),
    centralIdentityConfigured: () => true,
    loadCentralSession: () => ({ token: "token", person: { person_id: "person", display_name: "나", identity_kind: "google" }, device_id: "device", expires_at: 9_999_999_999 }),
    isCentralAuthenticationError: () => false,
  };
});

vi.mock("../lib/central/memberAutoAttach", () => ({
  attachMemberHost: (...args: unknown[]) => attach(...args),
  detachMemberHost: vi.fn(),
  MemberAnchorMismatchError: class MemberAnchorMismatchError extends Error {},
}));

const person = { person_id: "person", display_name: "나", identity_kind: "google" as const };
const memberServer = () => ({
  server_id: "friend",
  registration_epoch: "epoch",
  relation: "member" as const,
  alias: "친구",
  host_key_fingerprint: "fingerprint",
  host_public_key_jwk: {},
  endpoint: {
    origin: "https://friend.test",
    generation: 3,
    status: "published" as const,
    mode: "event_secure_v1",
    protocol: "secure_admission_v1",
  },
});

function bootstrapPayload() {
  return { person, servers: [memberServer()], server_time: 1 };
}

describe("member flood governor (real hooks, stub central)", () => {
  beforeEach(() => {
    cleanup();
    window.localStorage.clear();
    window.sessionStorage.clear();
    vi.clearAllMocks();
    bootstrap.mockReset();
    attach.mockReset();
    resetCentralRequestGovernorForTests();
    vi.useRealTimers();
  });

  it("stays within central budgets across 10 simulated minutes of 409 then 429", async () => {
    vi.useFakeTimers();
    try {
      let previewCalls = 0;
      let bootstrapCalls = 0;
      bootstrap.mockImplementation(async () => {
        bootstrapCalls += 1;
        acquireCentralPermit("/v1/bootstrap", "person");
        return bootstrapPayload();
      });
      attach.mockImplementation(async () => {
        previewCalls += 1;
        // The real attach spends one member-preview through the shared
        // signedFetch owner; the stub central answers 409 (revoked) for the
        // first 5 minutes and 429 afterwards, so no grant follows.
        acquireCentralPermit("/v1/servers/friend/member-preview", "person");
        if (previewCalls <= 3) {
          throw Object.assign(new Error("revoked"), { status: 409, code: "incarnation_conflict" });
        }
        const retryAfterMs = 120_000;
        const { noteCentral429 } = await import("../lib/central/centralRequestGovernor");
        noteCentral429("/v1/servers/friend/member-preview", "person", retryAfterMs);
        throw Object.assign(new Error("limited"), { status: 429, code: "rate_limited", retryAfterMs });
      });

      const { unmount } = renderHook(() => {
        const central = useCentralDirectory(true);
        const hosts = useMemberHosts({
          personId: central.directory?.person?.person_id ?? "person",
          sessionToken: "token",
          directory: central.directory,
        });
        const live = useMemberRoomLive({
          snapshots: hosts.snapshots,
          admissionFor: hosts.admissionFor,
          activeServerId: "",
          activeMeetingId: "",
        });
        return { central, hosts, live };
      });

      // Startup: one bootstrap, one preview attempt.
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });

      // 10 simulated minutes: revoked endpoint (slow cadence) then 429s
      // (Retry-After). Rapid React churn must not add attempts.
      for (let minute = 0; minute < 10; minute += 1) {
        await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
      }

      // Real-hook counts against the stub central. Budgets: at most 6
      // previews/host/10min and at most 20 central req/min total — over 10
      // minutes the whole app must stay far below 60/min sustained.
      // eslint-disable-next-line no-console
      console.log(`[flood-repro] 10min: preview=${previewCalls} bootstrap=${bootstrapCalls}`);
      expect(previewCalls).toBeLessThanOrEqual(6);
      expect(bootstrapCalls).toBeLessThanOrEqual(3);
      expect(previewCalls + bootstrapCalls).toBeLessThanOrEqual(9);
      unmount();
    } finally {
      vi.useRealTimers();
    }
  });

  it("stays within budgets during 2 minutes of focus churn while central is 429", async () => {
    vi.useFakeTimers();
    try {
      let previewNetwork = 0;
      let bootstrapNetwork = 0;
      let bootstrapAttempts = 0;
      const { noteCentral429 } = await import("../lib/central/centralRequestGovernor");
      const { CentralTemporaryError } = await import("../lib/central/connectionError");
      bootstrap.mockImplementation(async () => {
        bootstrapAttempts += 1;
        acquireCentralPermit("/v1/bootstrap", "person");
        bootstrapNetwork += 1;
        // Stub central keeps answering 429 with Retry-After: directory stays
        // unconfirmed so every focus tries a refresh; the governor must cap
        // the network requests.
        noteCentral429("/v1/bootstrap", "person", 60_000);
        throw new CentralTemporaryError("limited");
      });
      attach.mockImplementation(async () => {
        acquireCentralPermit("/v1/servers/friend/member-preview", "person");
        previewNetwork += 1;
        throw Object.assign(new Error("revoked"), { status: 409, code: "incarnation_conflict" });
      });

      const { result, unmount } = renderHook(() => {
        const central = useCentralDirectory(true);
        const hosts = useMemberHosts({
          personId: central.directory?.person?.person_id ?? "person",
          sessionToken: "token",
          directory: central.directory,
        });
        const live = useMemberRoomLive({
          snapshots: hosts.snapshots,
          admissionFor: hosts.admissionFor,
          activeServerId: "",
          activeMeetingId: "",
        });
        return { central, hosts, live };
      });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      const basePreview = previewNetwork;
      const baseBootstrap = bootstrapNetwork;

      // Directory refresh on focus every 3 s for 2 simulated minutes.
      for (let second = 0; second < 120; second += 3) {
        await act(async () => {
          window.dispatchEvent(new Event("focus"));
          await vi.advanceTimersByTimeAsync(3_000);
        });
      }

      const previews = previewNetwork - basePreview;
      const bootstraps = bootstrapNetwork - baseBootstrap;
      // eslint-disable-next-line no-console
      console.log(`[flood-repro] 2min-churn-429: attempts=${bootstrapAttempts} previewNet=${previews} bootstrapNet=${bootstraps}`);
      expect(previews).toBeLessThanOrEqual(2);
      expect(bootstraps).toBeLessThanOrEqual(3);
      expect(previews + bootstraps).toBeLessThanOrEqual(5);
      unmount();
      expect(result).toBeDefined();
    } finally {
      vi.useRealTimers();
    }
  });
});
