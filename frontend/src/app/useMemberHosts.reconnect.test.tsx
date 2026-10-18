import { act, cleanup, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useMemberHosts } from "./useMemberHosts";
import { saveMemberAnchor } from "../lib/central/memberAnchors";
import {
  MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP,
  memberHostOfflineBudgetExhausted,
  resetMemberHostOfflineBudgetForTests,
} from "../lib/central/memberRoomLive";
import {
  currentMemberHostReselectGeneration,
  resetMemberRoomAutoReselectBudgetForTests,
} from "../lib/central/memberRoomAutoReselectBudget";
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
    resetMemberHostOfflineBudgetForTests();
    resetMemberRoomAutoReselectBudgetForTests();
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("live room -> transport loss -> republish inside the fast slot reconnects in one preview and re-arms reselect", async () => {
    vi.useFakeTimers();
    try {
      // Deterministic jitter: the fast origin lookup lands at 22.5 s and the
      // slow ladder steps stay exact.
      vi.spyOn(Math, "random").mockReturnValue(0.5);
      saveMemberAnchor("person", {
        server_id: "friend",
        registration_epoch: "epoch",
        host_key_fingerprint: "fingerprint",
        endpoint_origin: "https://old.test",
        endpoint_generation: 3,
        rooms: [{ room_id: "general", name: "일반" }],
        hidden: false,
        updated_at: 1,
      });
      const live = controllableAdmission([{ room_id: "general", name: "일반" }]);
      const republished = controllableAdmission([{ room_id: "general", name: "일반" }]);
      let call = 0;
      attach.mockImplementation(async () => {
        call += 1;
        if (call === 1) {
          return {
            admission: live.admission,
            rooms: [{ room_id: "general", name: "일반" }],
            anchor: { server_id: "friend", registration_epoch: "epoch", endpoint_origin: "https://old.test" },
          };
        }
        return {
          admission: republished.admission,
          rooms: [{ room_id: "general", name: "일반" }],
          anchor: { server_id: "friend", registration_epoch: "epoch", endpoint_origin: "https://new.test" },
        };
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
      expect(result.current.snapshots[0]?.phase).toBe("connected");
      expect(attach).toHaveBeenCalledTimes(1);
      expect(currentMemberHostReselectGeneration("person", "friend")).toBe(1);

      // 09:36:06 the friend's host app restarts: the live channel drops. The
      // host republishes its new public origin ~11 s later, inside the fast
      // slot, so the single central lookup finds it.
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

      // Nothing fires before the fast slot opens (15 s): a restarting host
      // gets time to publish instead of an immediate central lookup.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(14_000);
      });
      expect(attach).toHaveBeenCalledTimes(1);

      // The one fast origin lookup lands at 22.5 s and reconnects on the
      // republished origin: exactly one member-preview, no slow-ladder wait.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(9_000);
      });
      expect(attach).toHaveBeenCalledTimes(2);
      expect(result.current.snapshots[0]?.phase).toBe("connected");
      expect(result.current.hostDirs[0]).toMatchObject({
        serverId: "friend",
        state: "connected",
        serverOrigin: "https://new.test",
      });
      // The fresh external admission releases exactly one automatic room
      // re-select, so the open friend room comes back within the minute.
      expect(currentMemberHostReselectGeneration("person", "friend")).toBe(2);
      // No repeat fast lookup and no loop behind the reconnect.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(20_000);
      });
      expect(attach).toHaveBeenCalledTimes(2);
    } finally {
      vi.restoreAllMocks();
      vi.useRealTimers();
    }
  });

  it("host stays down: one fast preview, then the slow ladder without extra previews", async () => {
    vi.useFakeTimers();
    try {
      vi.spyOn(Math, "random").mockReturnValue(0.5);
      saveMemberAnchor("person", {
        server_id: "friend",
        registration_epoch: "epoch",
        host_key_fingerprint: "fingerprint",
        endpoint_origin: "https://old.test",
        endpoint_generation: 3,
        rooms: [{ room_id: "general", name: "일반" }],
        hidden: false,
        updated_at: 1,
      });
      const live = controllableAdmission([{ room_id: "general", name: "일반" }]);
      let call = 0;
      attach.mockImplementation(async () => {
        call += 1;
        if (call === 1) {
          return {
            admission: live.admission,
            rooms: [{ room_id: "general", name: "일반" }],
            anchor: { server_id: "friend", registration_epoch: "epoch", endpoint_origin: "https://old.test" },
          };
        }
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
      expect(result.current.snapshots[0]?.phase).toBe("connected");

      await act(async () => {
        live.loseTransport();
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");

      // No central lookup in the first 14 s.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(14_000);
      });
      expect(attach).toHaveBeenCalledTimes(1);
      // The single fast preview fires at 22.5 s.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(9_000);
      });
      expect(attach).toHaveBeenCalledTimes(2);
      // The slow ladder's first step (+60 s) is still quiet up to 82 s...
      await act(async () => {
        await vi.advanceTimersByTimeAsync(59_000);
      });
      expect(attach).toHaveBeenCalledTimes(2);
      // ...and steps at 82.5 s: exactly one fast preview, then the ladder.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000);
      });
      expect(attach).toHaveBeenCalledTimes(3);
      expect(result.current.snapshots[0]?.phase).toBe("disconnected");
      expect(result.current.hostDirs[0]?.rooms).toHaveLength(1);
    } finally {
      vi.restoreAllMocks();
      vi.useRealTimers();
    }
  });

  it("keeps a live-loss reconnect inside the per-host daily preview cap over 6 hours", async () => {
    vi.useFakeTimers();
    try {
      // Noon UTC so the 6 h window never crosses the midnight reset.
      vi.setSystemTime(Date.UTC(2026, 9, 17, 12, 0, 0));
      vi.spyOn(Math, "random").mockReturnValue(0.5);
      saveMemberAnchor("person", {
        server_id: "friend",
        registration_epoch: "epoch",
        host_key_fingerprint: "fingerprint",
        endpoint_origin: "https://old.test",
        endpoint_generation: 3,
        rooms: [{ room_id: "general", name: "일반" }],
        hidden: false,
        updated_at: 1,
      });
      const live = controllableAdmission([{ room_id: "general", name: "일반" }]);
      let call = 0;
      attach.mockImplementation(async () => {
        call += 1;
        // Every preview spends one governed member-preview like the real
        // attach path through the shared signedFetch owner.
        acquireCentralPermit("/v1/servers/friend/member-preview", "person");
        if (call === 1) {
          return {
            admission: live.admission,
            rooms: [{ room_id: "general", name: "일반" }],
            anchor: { server_id: "friend", registration_epoch: "epoch", endpoint_origin: "https://old.test" },
          };
        }
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
      expect(result.current.snapshots[0]?.phase).toBe("connected");

      await act(async () => {
        live.loseTransport();
      });
      await act(async () => {
        await vi.advanceTimersByTimeAsync(0);
      });

      for (let hour = 0; hour < 6; hour += 1) {
        await act(async () => {
          await vi.advanceTimersByTimeAsync(3_600_000);
        });
      }
      // The successful live connect is not a preview; every retry is.
      const previews = attach.mock.calls.length - 1;
      // eslint-disable-next-line no-console
      console.log(`[reconnect-repro] 6h-live-loss: previews=${previews}`);
      expect(previews).toBeLessThanOrEqual(MEMBER_HOST_OFFLINE_DAILY_PREVIEW_CAP);
      expect(memberHostOfflineBudgetExhausted("person", "friend")).toBe(true);
      expect(result.current.hostDirs[0]?.rooms).toHaveLength(1);
    } finally {
      vi.restoreAllMocks();
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
