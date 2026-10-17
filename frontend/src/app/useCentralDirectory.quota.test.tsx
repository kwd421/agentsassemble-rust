import { act, cleanup, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCentralDirectory } from "./useCentralDirectory";
import { useMemberHosts } from "./useMemberHosts";
import { CentralTemporaryError } from "../lib/central/connectionError";
import { saveMemberAnchor } from "../lib/central/memberAnchors";
import {
  centralDailyQuotaNotice,
  centralDailyQuotaResetAtMs,
  centralDailyQuotaWaitMs,
  noteCentralDailyQuotaExhausted,
  resetCentralRequestGovernorForTests,
} from "../lib/central/centralRequestGovernor";

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

function saveFriendAnchor() {
  saveMemberAnchor("person", {
    server_id: "friend",
    registration_epoch: "epoch",
    host_key_fingerprint: "fingerprint",
    endpoint_origin: "https://friend.test",
    endpoint_generation: 2,
    rooms: [{ room_id: "general", name: "일반" }],
    hidden: false,
    updated_at: 1,
  });
}

const admissionFor = (rooms: { room_id: string; name: string }[]) => ({
  transport: { onClose: vi.fn(), active: true },
  close: vi.fn(),
  select: vi.fn(),
  rooms,
});

describe("daily central quota refusal (real hooks, stub central)", () => {
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

  it("sends nothing for 20 hours after a startup quota refusal, then reconnects with one retry", async () => {
    vi.useFakeTimers();
    try {
      // Start 23 hours before the UTC-midnight reset so the 20-hour parked
      // window below never crosses it.
      vi.setSystemTime(new Date(Date.UTC(2026, 9, 17, 1, 0, 0)));
      saveFriendAnchor();
      let bootstrapNetwork = 0;
      let previewNetwork = 0;
      let quotaSeen = false;
      const { acquireCentralPermit } =
        await import("../lib/central/centralRequestGovernor");
      bootstrap.mockImplementation(async () => {
        acquireCentralPermit("/v1/bootstrap", "person");
        bootstrapNetwork += 1;
        if (!quotaSeen) {
          quotaSeen = true;
          // The shared central response owner records actor_quota_exhausted
          // (429, no Retry-After) before the failure reaches the schedules.
          noteCentralDailyQuotaExhausted("person");
          throw Object.assign(new CentralTemporaryError(centralDailyQuotaNotice("person") ?? "limited"),
            { status: 429, code: "actor_quota_exhausted", retryAfterMs: centralDailyQuotaWaitMs("person") });
        }
        return bootstrapPayload();
      });
      attach.mockImplementation(async () => {
        acquireCentralPermit("/v1/servers/friend/member-preview", "person");
        previewNetwork += 1;
        return {
          admission: admissionFor([{ room_id: "general", name: "일반" }]),
          rooms: [{ room_id: "general", name: "일반" }],
          anchor: { server_id: "friend", registration_epoch: "epoch" },
        };
      });

      const { result, unmount } = renderHook(() => {
        const central = useCentralDirectory(true);
        const hosts = useMemberHosts({
          personId: central.directory?.person?.person_id ?? "person",
          sessionToken: "token",
          directory: central.directory,
        });
        return { central, hosts };
      });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(bootstrapNetwork).toBe(1);
      expect(result.current.central.directory?.status).toBe("central-unconfirmed");
      expect(centralDailyQuotaResetAtMs("person")).toBe(Date.UTC(2026, 9, 18));
      // The joined host waits with the reset-time reason, not silence.
      const parked = result.current.hosts.snapshots.find((entry) => entry.serverId === "friend");
      expect(parked?.phase).toBe("disconnected");
      expect(parked?.detail).toBe(centralDailyQuotaNotice("person"));

      // 20 simulated hours with no user action: zero further central requests.
      for (let hour = 0; hour < 20; hour += 1) {
        await act(async () => { await vi.advanceTimersByTimeAsync(3_600_000); });
      }
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(bootstrapNetwork).toBe(1);
      expect(previewNetwork).toBe(0);
      expect(result.current.central.directory?.status).toBe("central-unconfirmed");
      expect(centralDailyQuotaNotice("person")).not.toBeNull();

      // Past the reset (plus the 0-60 s jitter): exactly one bounded retry
      // reconnects the joined host without user action.
      const resetAt = centralDailyQuotaResetAtMs("person") ?? Date.UTC(2026, 9, 18);
      await act(async () => { await vi.advanceTimersByTimeAsync(resetAt + 61_000 - Date.now()); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });

      expect(bootstrapNetwork).toBe(2);
      expect(previewNetwork).toBe(1);
      expect(result.current.central.directory?.status).toBe("connected");
      expect(result.current.hosts.snapshots.find((entry) => entry.serverId === "friend")?.phase).toBe("connected");
      expect(centralDailyQuotaNotice("person")).toBeNull();
      unmount();
    } finally {
      vi.useRealTimers();
    }
  });
});
