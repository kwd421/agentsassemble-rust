import { act, cleanup, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useCentralDirectory } from "./useCentralDirectory";
import { useMemberHosts } from "./useMemberHosts";
import { useMemberRoomLive } from "./useMemberRoomLive";
import { loadMemberAnchor, saveMemberAnchor } from "../lib/central/memberAnchors";
import { acquireCentralPermit, resetCentralRequestGovernorForTests } from "../lib/central/centralRequestGovernor";
import { resetMemberHostOfflineBudgetForTests } from "../lib/central/memberRoomLive";
import { resetMemberRoomAutoReselectBudgetForTests } from "../lib/central/memberRoomAutoReselectBudget";

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
const ORIGIN = "https://friend.test";
function memberServer(origin = ORIGIN) {
  return {
    server_id: "friend",
    registration_epoch: "epoch",
    relation: "member" as const,
    alias: "친구",
    host_key_fingerprint: "fingerprint",
    host_public_key_jwk: {},
    endpoint: { origin, generation: 3, status: "published" as const, mode: "event_secure_v1", protocol: "secure_admission_v1" },
  };
}
function bootstrapPayload(origin = ORIGIN) {
  return { person, servers: [memberServer(origin)], server_time: 1 };
}
function controllableAdmission(origin = ORIGIN) {
  const listeners = new Set<() => void>();
  const transport: { active: boolean; hello: { origin: string }; onClose: (l: () => void) => () => void; close: () => void } = {
    active: true,
    hello: { origin },
    onClose: vi.fn((l: () => void) => { listeners.add(l); return () => { listeners.delete(l); }; }),
    close: vi.fn(() => { transport.active = false; }),
  };
  return {
    admission: {
      transport,
      close: vi.fn(),
      select: vi.fn(),
      selectBackgroundRoom: vi.fn(async (roomId: string) => ({
        session_token: `session-${roomId}`,
        agent_id: "member-1",
        display_name: "Member",
        meeting_id: roomId,
        server_id: "friend",
        server_product_surface: { websocket_streams: ["room_events"] },
      })),
      deviceToken: "device",
    },
    loseTransport: () => {
      transport.active = false;
      for (const l of [...listeners]) l();
    },
  };
}

describe("idle connected member host costs nothing", () => {
  beforeEach(() => {
    cleanup();
    window.localStorage.clear();
    window.sessionStorage.clear();
    vi.clearAllMocks();
    bootstrap.mockReset();
    attach.mockReset();
    resetCentralRequestGovernorForTests();
    resetMemberHostOfflineBudgetForTests();
    resetMemberRoomAutoReselectBudgetForTests();
    vi.useRealTimers();
  });

  it("connected host with 2 rooms: 60min focus churn stays within one bootstrap per 5min and zero preview/grant", async () => {
    vi.useFakeTimers();
    try {
      vi.spyOn(Math, "random").mockReturnValue(0.5);
      saveMemberAnchor("person", {
        server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
        endpoint_origin: ORIGIN, endpoint_generation: 3,
        rooms: [{ room_id: "room-a", name: "A" }, { room_id: "room-b", name: "B" }],
        hidden: false, updated_at: 1,
      });
      let bootstrapCalls = 0;
      let previewCalls = 0;
      bootstrap.mockImplementation(async () => {
        bootstrapCalls += 1;
        acquireCentralPermit("/v1/bootstrap", "person");
        // Directory status flaps between publishes while the origin stays
        // put: a connected host must not re-attach (zero central) on it.
        const flapped = bootstrapCalls % 2 === 0 ? "likely_online" as const : "published" as const;
        const payload = bootstrapPayload();
        return { ...payload, servers: [{ ...payload.servers[0], endpoint: { ...payload.servers[0].endpoint, status: flapped } }] };
      });
      const live = controllableAdmission();
      attach.mockImplementation(async () => {
        previewCalls += 1;
        acquireCentralPermit("/v1/servers/friend/member-preview", "person");
        return {
          admission: live.admission,
          rooms: [{ room_id: "room-a", name: "A" }, { room_id: "room-b", name: "B" }],
          anchor: { server_id: "friend", registration_epoch: "epoch", endpoint_origin: ORIGIN },
        };
      });
      const openSocket = vi.fn(() => ({ close: vi.fn() }));
      const { result, unmount } = renderHook(() => {
        const central = useCentralDirectory(true);
        const hosts = useMemberHosts({ personId: central.directory?.person?.person_id ?? "person", sessionToken: "token", directory: central.directory });
        const roomLive = useMemberRoomLive({ snapshots: hosts.snapshots, admissionFor: hosts.admissionFor, activeServerId: "friend", activeMeetingId: "room-a", accountId: "person", openSocket: openSocket as never });
        return { central, hosts, roomLive };
      });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(1000); });
      expect(result.current.hosts.snapshots[0]?.phase).toBe("connected");
      const baseBootstrap = bootstrapCalls;
      const basePreview = previewCalls;
      const bgSelect = live.admission.selectBackgroundRoom as unknown as { mock: { calls: unknown[] } };
      const baseBgSelect = bgSelect.mock.calls.length;
      for (let minute = 0; minute < 60; minute += 1) {
        await act(async () => {
          window.dispatchEvent(new Event("focus"));
          vi.spyOn(document, "visibilityState", "get").mockReturnValue("visible");
          document.dispatchEvent(new Event("visibilitychange"));
          await vi.advanceTimersByTimeAsync(60_000);
        });
      }
      const windowBootstrap = bootstrapCalls - baseBootstrap;
      const windowPreview = previewCalls - basePreview;
      const windowBgSelect = bgSelect.mock.calls.length - baseBgSelect;
      expect(windowBootstrap).toBeLessThanOrEqual(12);
      expect(windowPreview).toBe(0);
      expect(windowBgSelect).toBe(0);
      expect(result.current.hosts.snapshots[0]?.phase).toBe("connected");
      unmount();
    } finally {
      vi.restoreAllMocks();
      vi.useRealTimers();
    }
  });

  it("re-attach to a new origin updates the stored anchor origin", async () => {
    vi.useFakeTimers();
    try {
      vi.spyOn(Math, "random").mockReturnValue(0.5);
      const OLD_ORIGIN = "https://concord-subscribe-friend.test";
      const NEW_ORIGIN = "https://pad-swaziland-friend.test";
      saveMemberAnchor("person", {
        server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
        endpoint_origin: OLD_ORIGIN, endpoint_generation: 3,
        rooms: [{ room_id: "room-a", name: "A" }],
        hidden: false, updated_at: 1,
      });
      bootstrap.mockImplementation(async () => {
        acquireCentralPermit("/v1/bootstrap", "person");
        return bootstrapPayload(NEW_ORIGIN);
      });
      const fresh = controllableAdmission(NEW_ORIGIN);
      attach.mockImplementation(async () => {
        acquireCentralPermit("/v1/servers/friend/member-preview", "person");
        const { saveMemberAnchor: save } = await import("../lib/central/memberAnchors");
        save("person", {
          server_id: "friend", registration_epoch: "epoch", host_key_fingerprint: "fingerprint",
          endpoint_origin: NEW_ORIGIN, endpoint_generation: 4,
          rooms: [{ room_id: "room-a", name: "A" }], hidden: false, updated_at: Date.now(),
        });
        return {
          admission: fresh.admission,
          rooms: [{ room_id: "room-a", name: "A" }],
          anchor: { server_id: "friend", registration_epoch: "epoch", endpoint_origin: NEW_ORIGIN },
        };
      });
      const { result, unmount } = renderHook(() => {
        const central = useCentralDirectory(true);
        const hosts = useMemberHosts({ personId: central.directory?.person?.person_id ?? "person", sessionToken: "token", directory: central.directory });
        return { central, hosts };
      });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      await act(async () => { await vi.advanceTimersByTimeAsync(0); });
      expect(result.current.hosts.snapshots[0]?.phase).toBe("connected");
      expect(result.current.hosts.snapshots[0]?.origin).toBe(NEW_ORIGIN);
      expect(loadMemberAnchor("person", "friend")?.endpoint_origin).toBe(NEW_ORIGIN);
      unmount();
    } finally {
      vi.restoreAllMocks();
      vi.useRealTimers();
    }
  });
});
