import { act, cleanup, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useMemberHosts } from "./useMemberHosts";
import { saveMemberAnchor } from "../lib/central/memberAnchors";
import {
  acquireCentralPermit,
  resetCentralRequestGovernorForTests,
} from "../lib/central/centralRequestGovernor";

const attach = vi.fn();
vi.mock("../lib/central/memberAutoAttach", () => ({
  attachMemberHost: (...args: unknown[]) => attach(...args),
  detachMemberHost: vi.fn(),
  MemberAnchorMismatchError: class MemberAnchorMismatchError extends Error {},
}));

type CloseListener = () => void;

function controllableAdmission(rooms: { room_id: string; name: string }[]) {
  const listeners = new Set<CloseListener>();
  const transport = {
    active: true,
    onClose: vi.fn((listener: CloseListener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    }),
    close: vi.fn(() => {
      transport.active = false;
    }),
  };
  return {
    admission: {
      transport,
      close: vi.fn(),
      select: vi.fn(),
    },
    rooms,
    loseTransport: () => {
      transport.active = false;
      for (const listener of [...listeners]) listener();
    },
  };
}

const liveServer = (overrides = {}) => ({
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
  ...overrides,
});

const directoryFor = (servers: ReturnType<typeof liveServer>[]) => ({
  status: "connected" as const,
  person: { person_id: "person", display_name: "나", identity_kind: "google" as const },
  servers: servers.map(
    ({ host_key_fingerprint: _key, host_public_key_jwk: _jwk, endpoint, ...rest }) => ({ ...rest }),
  ),
  live: {
    person: { person_id: "person", display_name: "나", identity_kind: "google" as const },
    servers,
    server_time: 1,
  },
  error: undefined,
});

function revokedEndpoint() {
  return Object.assign(new Error("revoked"), { status: 409, code: "incarnation_conflict" });
}

describe("joined friend host restart (real hook, fake timers)", () => {
  beforeEach(() => {
    cleanup();
    window.localStorage.clear();
    window.sessionStorage.clear();
    vi.clearAllMocks();
    attach.mockReset();
    resetCentralRequestGovernorForTests();
    vi.useRealTimers();
  });

  it("live room -> transport loss -> 409s while down -> republish reconnects on the slow cadence", async () => {
    vi.useFakeTimers();
    try {
      saveMemberAnchor("person", {
        server_id: "friend",
        registration_epoch: "epoch",
        host_key_fingerprint: "fingerprint",
        endpoint_origin: "https://friend.test",
        endpoint_generation: 3,
        rooms: [{ room_id: "general", name: "일반" }],
        hidden: false,
        updated_at: 1,
      });
      const live = controllableAdmission([{ room_id: "general", name: "일반" }]);
      attach.mockImplementation(async () => ({
        admission: live.admission,
        rooms: [{ room_id: "general", name: "일반" }],
        anchor: { server_id: "friend", registration_epoch: "epoch" },
      }));
      const { result } = renderHook(() => useMemberHosts({
        personId: "person",
        sessionToken: "token",
        directory: directoryFor([liveServer()]),
      }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(result.current.snapshots[0]?.phase).toBe("connected");

      // 16:29 the friend's host app quits: the live channel drops.
      attach.mockRejectedValue(revokedEndpoint());
      await act(async () => {
        live.loseTransport();
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      // The joined room stays listed from the anchor (grey dot + retry),
      // never removed while the host is merely down.
      expect(result.current.hostDirs[0]).toMatchObject({
        serverId: "friend",
        rooms: [{ room_id: "general", label: "일반" }],
        state: "disconnected",
      });
      const downAttempts = attach.mock.calls.length;

      // While the host is down every preview answers 409
      // incarnation_conflict: the slow cadence keeps retrying (bounded),
      // never storms and never parks in silence.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(75_000);
      });
      expect(attach.mock.calls.length).toBe(downAttempts + 1);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(150_000);
      });
      expect(attach.mock.calls.length).toBe(downAttempts + 2);
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      expect(result.current.hostDirs[0]?.rooms).toHaveLength(1);

      // 16:31 the host republishes its endpoint (new public origin). The
      // next slow-cadence attempt reconnects with no user action and no
      // directory refresh: the preview answers the fresh endpoint.
      const republished = controllableAdmission([{ room_id: "general", name: "일반" }]);
      attach.mockImplementation(async () => ({
        admission: republished.admission,
        rooms: [{ room_id: "general", name: "일반" }],
        anchor: { server_id: "friend", registration_epoch: "epoch" },
      }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(360_000);
      });
      expect(result.current.snapshots[0]?.phase).toBe("connected");
      expect(result.current.hostDirs[0]).toMatchObject({
        serverId: "friend",
        state: "connected",
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("stays within the governor budget across 60 simulated minutes of 409s", async () => {
    vi.useFakeTimers();
    try {
      saveMemberAnchor("person", {
        server_id: "friend",
        registration_epoch: "epoch",
        host_key_fingerprint: "fingerprint",
        endpoint_origin: "https://friend.test",
        endpoint_generation: 3,
        rooms: [{ room_id: "general", name: "일반" }],
        hidden: false,
        updated_at: 1,
      });
      let previewCalls = 0;
      attach.mockImplementation(async () => {
        previewCalls += 1;
        // Every attempt spends exactly one governed member-preview, like
        // the real attach path through the shared signedFetch owner.
        acquireCentralPermit("/v1/servers/friend/member-preview", "person");
        throw revokedEndpoint();
      });
      const { result } = renderHook(() => useMemberHosts({
        personId: "person",
        sessionToken: "token",
        directory: directoryFor([liveServer()]),
      }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");

      for (let minute = 0; minute < 60; minute += 1) {
        await act(async () => {
          await vi.advanceTimersByTimeAsync(60_000);
        });
      }
      // Slow cadence (60 s doubling to a 5 min cap with jitter): at most
      // 5 previews per 10 minutes, at most 18 per host per hour while
      // revoked — far inside the 6-per-10-min governor budget, so the
      // governor never throttles and the retry chain never parks.
      // eslint-disable-next-line no-console
      console.log(`[reconnect-repro] 60min-409: preview=${previewCalls}`);
      expect(previewCalls).toBeLessThanOrEqual(18);
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      expect(result.current.hostDirs[0]?.rooms).toHaveLength(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps an identity refusal terminal while 409s keep retrying", async () => {
    vi.useFakeTimers();
    try {
      saveMemberAnchor("person", {
        server_id: "friend",
        registration_epoch: "old-epoch",
        host_key_fingerprint: "fingerprint",
        endpoint_origin: "https://friend.test",
        endpoint_generation: 3,
        rooms: [{ room_id: "general", name: "일반" }],
        hidden: false,
        updated_at: 1,
      });
      attach.mockRejectedValue(revokedEndpoint());
      const { result } = renderHook(() => useMemberHosts({
        personId: "person",
        sessionToken: "token",
        directory: directoryFor([liveServer()]),
      }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      // Epoch changed: terminal refusal with the re-join reason, no attach.
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      expect(result.current.snapshots[0]?.detail).toMatch(/새 초대/);
      expect(attach).not.toHaveBeenCalled();
      const calls = attach.mock.calls.length;
      await act(async () => {
        await vi.advanceTimersByTimeAsync(600_000);
      });
      expect(attach.mock.calls.length).toBe(calls);
    } finally {
      vi.useRealTimers();
    }
  });
});
