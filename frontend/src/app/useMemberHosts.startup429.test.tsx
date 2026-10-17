import { act, cleanup, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCentralDirectory } from "./useCentralDirectory";
import { useMemberHosts } from "./useMemberHosts";
import { CentralTemporaryError } from "../lib/central/connectionError";
import { saveMemberAnchor } from "../lib/central/memberAnchors";
import { resetCentralRequestGovernorForTests } from "../lib/central/centralRequestGovernor";

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

describe("startup central 429 reconnect (real hooks, stub central)", () => {
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

  it("retries the directory after the governor block and connects the joined host without user action", async () => {
    vi.useFakeTimers();
    try {
      saveFriendAnchor();
      let bootstrapNetwork = 0;
      let bootstrapCalls = 0;
      const { acquireCentralPermit, noteCentral429 } =
        await import("../lib/central/centralRequestGovernor");
      bootstrap.mockImplementation(async () => {
        bootstrapCalls += 1;
        acquireCentralPermit("/v1/bootstrap", "person");
        bootstrapNetwork += 1;
        if (bootstrapCalls === 1) {
          // Startup: central answers 429 with Retry-After. The governor
          // parks this family plus the global lane for ~60 s.
          noteCentral429("/v1/bootstrap", "person", 60_000);
          throw Object.assign(new CentralTemporaryError("limited"),
            { status: 429, code: "rate_limited", retryAfterMs: 60_000 });
        }
        return bootstrapPayload();
      });
      attach.mockImplementation(async () => {
        acquireCentralPermit("/v1/servers/friend/member-preview", "person");
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

      // No user action at all: no focus/online events, no retryHost call.
      // Past the 60 s governor block the directory must retry on its own
      // and the anchored host must attach from its anchor.
      await act(async () => { await vi.advanceTimersByTimeAsync(61_000); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(5_000); });

      expect(bootstrapNetwork).toBeGreaterThanOrEqual(2);
      expect(attach).toHaveBeenCalled();
      expect(result.current.central.directory?.status).toBe("connected");
      expect(result.current.hosts.snapshots.find((entry) => entry.serverId === "friend")?.phase).toBe("connected");
      unmount();
    } finally {
      vi.useRealTimers();
    }
  });

  it("stays within governor budgets across 30 simulated minutes of central 429", async () => {
    vi.useFakeTimers();
    try {
      saveFriendAnchor();
      let bootstrapNetwork = 0;
      let previewNetwork = 0;
      const { acquireCentralPermit, noteCentral429 } =
        await import("../lib/central/centralRequestGovernor");
      bootstrap.mockImplementation(async () => {
        acquireCentralPermit("/v1/bootstrap", "person");
        bootstrapNetwork += 1;
        noteCentral429("/v1/bootstrap", "person", 60_000);
        throw Object.assign(new CentralTemporaryError("limited"),
          { status: 429, code: "rate_limited", retryAfterMs: 60_000 });
      });
      attach.mockImplementation(async () => {
        acquireCentralPermit("/v1/servers/friend/member-preview", "person");
        previewNetwork += 1;
        noteCentral429("/v1/servers/friend/member-preview", "person", 60_000);
        throw Object.assign(new Error("limited"),
          { status: 429, code: "rate_limited", retryAfterMs: 60_000 });
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

      for (let minute = 0; minute < 30; minute += 1) {
        await act(async () => { await vi.advanceTimersByTimeAsync(60_000); });
      }
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });

      // Liveness: retries happen on their own. Budget: far below the
      // governor's 20 central req/min total and 6 previews/host/10 min
      // (which would allow ~600 and ~18 over 30 minutes).
      // eslint-disable-next-line no-console
      console.log(`[storm-repro] 30min-429: bootstrapNet=${bootstrapNetwork} previewNet=${previewNetwork} attachCalls=${attach.mock.calls.length}`);
      expect(bootstrapNetwork).toBeGreaterThanOrEqual(2);
      expect(bootstrapNetwork).toBeLessThanOrEqual(8);
      expect(previewNetwork).toBeLessThanOrEqual(8);
      expect(attach.mock.calls.length).toBeLessThanOrEqual(12);
      expect(bootstrapNetwork + previewNetwork).toBeLessThanOrEqual(12);
      expect(result.current.central.directory?.status).toBe("central-unconfirmed");
      unmount();
    } finally {
      vi.useRealTimers();
    }
  });
});
