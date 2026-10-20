import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Hash } from "../ui/icons";
import { useAppController } from "./useAppController";
import type { CentralDirectoryState } from "./useCentralDirectory";
import { bindRemoteSession, closeRemoteWorkspace, installRemoteWorkspace, remoteSessionTransport, unbindRemoteSessionsForServer } from "../lib/remote/remoteWorkspace";
import type { RemoteTransport } from "../lib/remote/remoteTransport";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { RoomDirectoryContinuity } from "./useRoomDirectory";
import type { RoomInviteJoinResponse } from "../lib/roomAdmissionContract";
import type { MemberHostAnchor } from "../lib/central/memberAnchors";
import { roomGuestSessionFromJoinPayload } from "../lib/roomGuestSession";
import { useMemberHosts } from "./useMemberHosts";
import { saveMemberAnchor } from "../lib/central/memberAnchors";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";

const hooks = vi.hoisted(() => ({
  directory: null as CentralDirectoryState | null,
  refresh: vi.fn(),
  opens: [] as Array<{ roomId: string; auth: unknown; serverSurface: unknown }>,
  closes: [] as Array<{ roomId: string; auth: unknown; serverSurface: unknown }>,
  attach: vi.fn(),
  selectOld: vi.fn(),
  selectNew: vi.fn(),
  fetchRoomSettings: vi.fn(),
  initialRooms: [] as RoomDockItem[],
  alert: vi.fn(),
}));

const OWN_MEETING = "own-1";
const MEMBER_MEETING = "friend-room";
const MEMBER_SERVER = "friend-server";
const OLD_TOKEN = "member-token-old";
const NEW_TOKEN = "member-token-new";
const OLD_ORIGIN = "https://layout-campaigns-performs-bias.trycloudflare.com";
const NEW_ORIGIN = "https://fitness-this-grass-spas.trycloudflare.com";
const MEMBER_SURFACE = { ...TEST_SERVER_PRODUCT_SURFACE };

vi.mock("./useCentralDirectory", () => ({
  useCentralDirectory: () => ({ directory: hooks.directory, refresh: hooks.refresh }),
}));

vi.mock("./useRoomDirectory", async () => {
  const React = await import("react");
  const dock = await import("../lib/roomDockModel");
  return {
    useRoomDirectory: () => {
      const [rooms, setRooms] = React.useState<RoomDockItem[]>(() => [...hooks.initialRooms]);
      const replaceRooms = React.useCallback((next: RoomDockItem[]) => setRooms([...next]), []);
      const mergeFlowRoom = React.useCallback((room: RoomDockItem | null) => {
        if (!room) return;
        setRooms((current) => {
          const index = current.findIndex((existing) =>
            dock.roomDockIdentity(existing) === dock.roomDockIdentity(room) ||
            (existing.roomOrigin === "remote_server" && room.roomOrigin === "remote_server" &&
              Boolean(existing.serverId) && existing.serverId === room.serverId &&
              existing.meetingId === room.meetingId));
          if (index >= 0) {
            const next = [...current];
            next[index] = { ...next[index], topic: room.topic };
            return next;
          }
          const [first, ...rest] = current;
          return first ? [first, room, ...rest] : [room];
        });
      }, []);
      const removeRoom = React.useCallback((roomId: string) => {
        let next: RoomDockItem[] = [];
        setRooms((current) => {
          next = current.filter((room) => room.id !== roomId);
          return next;
        });
        return next;
      }, []);
      return {
        rooms, managementRooms: [], ownerProfileRevision: 0, hostingRestriction: null,
        replaceRooms, prependRoom: (room: RoomDockItem) => setRooms((current) => [room, ...current]),
        mergeFlowRoom, removeRoom,
        updateRoom: (roomId: string, updates: Partial<RoomDockItem>) =>
          setRooms((current) => current.map((room) => (room.id === roomId ? { ...room, ...updates } : room))),
        updateRoomByMeetingId: (meetingId: string, updates: Partial<RoomDockItem>) =>
          setRooms((current) => current.map((room) => (room.meetingId === meetingId ? { ...room, ...updates } : room))),
        captureRoomDirectoryContinuity: () => ({} as unknown as RoomDirectoryContinuity),
        validateRoomDirectoryContinuity: () => undefined,
        refreshRoomDirectory: async () => ({ ok: true as const, rooms, continuity: ({} as unknown as RoomDirectoryContinuity) }),
        verifyRoomDirectoryAuthority: async () => ({ ok: true as const, continuity: ({} as unknown as RoomDirectoryContinuity) }),
        managerAuthorityCurrent: false,
        resolveManagerRoomAuthority: () => { throw new Error("no manager authority in this test"); },
        syncIssue: null,
      };
    },
  };
});

vi.mock("../roomSocketClient", async () => {
  const actual = await vi.importActual<typeof import("../roomSocketClient")>("../roomSocketClient");
  return {
    ...actual,
    openRoomSocket: (
      auth: unknown,
      _streams: string[],
      _handlers: unknown,
      dependencies: { expectedRoomId: string; serverSurface?: unknown },
    ) => {
      const opened = { roomId: dependencies.expectedRoomId, auth, serverSurface: dependencies.serverSurface };
      hooks.opens.push(opened);
      return {
        ready: () => true,
        readyState: 1,
        resync: () => undefined,
        close: () => { hooks.closes.push(opened); },
        command: async () => ({ result: {} }),
        historyBefore: async () => ({ room_id: dependencies.expectedRoomId, channel_id: "lobby", events: [], oldest_seq: 0, last_seq: 0, has_more_before: false }),
      };
    },
  };
});

vi.mock("../lib/central/memberAutoAttach", () => ({
  attachMemberHost: (...args: unknown[]) => hooks.attach(...args),
  detachMemberHost: vi.fn(),
  MemberAnchorMismatchError: class MemberAnchorMismatchError extends Error {},
}));

vi.mock("../lib/roomDirectoryContract", async () => ({
  ...(await vi.importActual<typeof import("../lib/roomDirectoryContract")>("../lib/roomDirectoryContract")),
  verifyAndBindRoomSessionSurface: vi.fn().mockResolvedValue(true),
  currentServerProductSurface: () => TEST_SERVER_PRODUCT_SURFACE,
}));

vi.mock("../lib/desktopBridge", async () => ({
  ...(await vi.importActual<typeof import("../lib/desktopBridge")>("../lib/desktopBridge")),
  isDesktopWebview: () => true,
}));

vi.mock("../api", async () => ({
  ...(await vi.importActual<typeof import("../api")>("../api")),
  fetchRoomSettings: (...args: unknown[]) => hooks.fetchRoomSettings(...args),
  saveRoomSettings: vi.fn(),
}));

function directoryFor(origin: string): CentralDirectoryState {
  const server = {
    server_id: MEMBER_SERVER, registration_epoch: "epoch-1", relation: "member" as const,
    alias: "Friend", host_key_fingerprint: "fp", host_public_key_jwk: {},
    endpoint: { origin, generation: origin === NEW_ORIGIN ? 4 : 3, status: "published" as const, mode: "event_secure_v1", protocol: "secure_admission_v1" },
  };
  return {
    status: "connected",
    person: { person_id: "person", display_name: "나", identity_kind: "google" },
    servers: [],
    live: { person: { person_id: "person", display_name: "나", identity_kind: "google" }, servers: [server], server_time: 1 },
  } as unknown as CentralDirectoryState;
}

function payloadFor(token: string): RoomInviteJoinResponse {
  return {
    status: "admitted", request_id: `req-${token}`, session_token: token, agent_id: "member-agent",
    display_name: "Friend Member", meeting_id: MEMBER_MEETING, invite_scope: "room",
    participant_type: "human", client_type: "browser", provider_kind: "", connection_kind: "",
    expires_at: "2099-01-01T00:00:00Z", room_label: "Friend room", room_topic: "",
    room_created_at: "", server_id: MEMBER_SERVER,
    authority_lineage_id: "lineage", server_product_surface: MEMBER_SURFACE,
    owner_display_name: "", owner_id: "", stable_identity: false, operator: false,
    client_id: "client-test",
    guide: { welcome: "", how_to: [], etiquette: [], session: { expires_in_seconds: 0, rejoin: "" } },
  };
}

function fakeTransport(origin: string) {
  const listeners = new Set<() => void>();
  const transport = {
    hello: { origin, server_id: MEMBER_SERVER },
    active: true,
    onClose: vi.fn((listener: () => void) => {
      listeners.add(listener);
      return () => { listeners.delete(listener); };
    }),
    close: vi.fn(() => {
      (transport as { active: boolean }).active = false;
      for (const listener of [...listeners]) listener();
    }),
    fetch: () => Promise.reject(new Error("no fetch in this test")),
  } as unknown as RemoteTransport & { active: boolean };
  return transport;
}

function installPayload(token: string, transport: RemoteTransport) {
  const payload = payloadFor(token);
  const member = { ...roomGuestSessionFromJoinPayload("", payload), centralMember: true as const };
  bindRemoteSession(member.sessionToken, transport, member.serverSurface.server_id);
  installRemoteWorkspace({ transport, owner: null, member, deviceToken: "device-test", clientId: "client-test" });
  return payload;
}

describe("member host restart on a new origin (real hooks, fake transports)", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    closeRemoteWorkspace();
    unbindRemoteSessionsForServer(MEMBER_SERVER);
    vi.clearAllMocks();
    hooks.opens.length = 0;
    hooks.closes.length = 0;
    window.alert = hooks.alert;
    window.localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({
      token: "central-token", expires_at: 9_999_999_999, device_id: "device",
      person: { person_id: "person", identity_kind: "google", display_name: "나" },
    }));
    hooks.directory = directoryFor(OLD_ORIGIN);
    hooks.initialRooms = [{
      id: "own-room", label: "새 방", meetingId: OWN_MEETING, roomOrigin: "local",
      connectionState: "local", topic: "own", shortLabel: "새", icon: Hash, createdAt: "", tone: "fresh",
    }];
    hooks.fetchRoomSettings.mockImplementation(async (room: RoomDockItem) => ({
      roomId: room.meetingId, label: room.label, topic: room.topic, shortLabel: room.shortLabel,
      appearance: { bannerPreset: "default", notifications: "default", inviteScope: "room" },
      channelSettings: {}, conversationMode: "ordered", toolMode: "chat", orderedExcludePreviousSpeaker: false,
    }));
  });

  afterEach(cleanup);

  it("re-selects the open room once on the new admission; socket uses the fresh session only", async () => {
    const oldTransport = fakeTransport(OLD_ORIGIN);
    const newTransport = fakeTransport(NEW_ORIGIN);
    const saveAnchor = async (origin: string) => {
      const anchor: MemberHostAnchor = {
        server_id: MEMBER_SERVER, registration_epoch: "epoch-1", host_key_fingerprint: "fp",
        endpoint_origin: origin, endpoint_generation: origin === NEW_ORIGIN ? 4 : 3,
        rooms: [{ room_id: MEMBER_MEETING, name: "Friend room" }], hidden: false, updated_at: Date.now(),
      };
      const { saveMemberAnchor } = await import("../lib/central/memberAnchors");
      saveMemberAnchor("person", anchor);
      return anchor;
    };
    let attachCalls = 0;
    const oldAdmission = {
      transport: oldTransport,
      deviceToken: "device-test",
      close: vi.fn(),
      select: (...args: unknown[]) => hooks.selectOld(...args),
    };
    const newAdmission = {
      transport: newTransport,
      deviceToken: "device-test",
      close: vi.fn(),
      select: (...args: unknown[]) => hooks.selectNew(...args),
    };
    hooks.selectOld.mockImplementation(async () => installPayload(OLD_TOKEN, oldTransport));
    hooks.selectNew.mockImplementation(async () => installPayload(NEW_TOKEN, newTransport));
    hooks.attach.mockImplementation(async () => {
      attachCalls += 1;
      if (attachCalls === 1) {
        return {
          admission: oldAdmission,
          rooms: [{ room_id: MEMBER_MEETING, name: "Friend room" }],
          anchor: await saveAnchor(OLD_ORIGIN),
        };
      }
      return {
        admission: newAdmission,
        rooms: [{ room_id: MEMBER_MEETING, name: "Friend room" }],
        anchor: await saveAnchor(NEW_ORIGIN),
      };
    });

    const { result, rerender } = renderHook(() => useAppController("device-test", "client-test"));
    await waitFor(() => {
      if (result.current.rooms.length !== 2) throw new Error("rail not merged");
    });
    const memberId = result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING)!.id;
    await waitFor(() => expect(result.current.activeRoom.meetingId).toBe(OWN_MEETING));

    await act(async () => { await result.current.selectRoom(memberId); });
    await waitFor(() => expect(result.current.activeRoom.meetingId).toBe(MEMBER_MEETING));
    expect(hooks.selectOld).toHaveBeenCalledTimes(1);
    expect(remoteSessionTransport(OLD_TOKEN)).toBeTruthy();
    const firstOpens = hooks.opens.filter((open) => open.roomId === MEMBER_MEETING);
    expect(firstOpens).toHaveLength(1);
    expect(firstOpens[0].auth).toMatchObject({ kind: "session", sessionToken: OLD_TOKEN });

    // Friend host restarts on a new public origin: the old channel dies and
    // the directory republishes the fresh origin. The member re-attaches
    // (second attach) without user action.
    await act(async () => {
      oldTransport.active = false;
      (oldTransport.close as unknown as () => void)();
    });
    hooks.directory = directoryFor(NEW_ORIGIN);
    rerender();
    await waitFor(() => expect(attachCalls).toBe(2), { timeout: 5000 });
    // The open room re-selects once on the new admission.
    await waitFor(() => expect(hooks.selectNew).toHaveBeenCalledTimes(1), { timeout: 5000 });
    await waitFor(() => {
      const opens = hooks.opens.filter((open) => open.roomId === MEMBER_MEETING);
      if (!opens.some((open) => (open.auth as { sessionToken?: string }).sessionToken === NEW_TOKEN)) {
        throw new Error("socket has not switched to the fresh session yet");
      }
    });
    // Stale bearer is dropped: the old session never drives a new socket,
    // and the re-select does not spin.
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 100)); });
    expect(hooks.selectNew).toHaveBeenCalledTimes(1);
    expect(hooks.selectOld).toHaveBeenCalledTimes(1);
    const memberOpens = hooks.opens.filter((open) => open.roomId === MEMBER_MEETING);
    expect(memberOpens[memberOpens.length - 1].auth).toMatchObject({ kind: "session", sessionToken: NEW_TOKEN });
    const firstNewIndex = memberOpens.findIndex((open) => (open.auth as { sessionToken?: string }).sessionToken === NEW_TOKEN);
    expect(firstNewIndex).toBeGreaterThanOrEqual(0);
    expect(memberOpens.slice(firstNewIndex).every((open) => (open.auth as { sessionToken?: string }).sessionToken === NEW_TOKEN)).toBe(true);
    expect(result.current.memberSelectError).toBeNull();
  });

  it("sign-in with an existing anchor and a reachable host connects without user action", async () => {
    saveMemberAnchor("person", {
      server_id: MEMBER_SERVER, registration_epoch: "epoch-1", host_key_fingerprint: "fp",
      endpoint_origin: OLD_ORIGIN, endpoint_generation: 3,
      rooms: [{ room_id: MEMBER_MEETING, name: "Friend room" }], hidden: false, updated_at: 1,
    });
    const transport = fakeTransport(OLD_ORIGIN);
    hooks.attach.mockImplementation(async () => ({
      admission: { transport, close: vi.fn(), select: vi.fn() },
      rooms: [{ room_id: MEMBER_MEETING, name: "Friend room" }],
      anchor: {
        server_id: MEMBER_SERVER, registration_epoch: "epoch-1", host_key_fingerprint: "fp",
        endpoint_origin: OLD_ORIGIN, endpoint_generation: 3,
        rooms: [{ room_id: MEMBER_MEETING, name: "Friend room" }], hidden: false, updated_at: Date.now(),
      },
    }));
    const { result } = renderHook(() => useMemberHosts({
      personId: "person",
      sessionToken: "token",
      directory: directoryFor(OLD_ORIGIN),
    }));
    await waitFor(() => expect(result.current.snapshots[0]?.phase).toBe("connected"), { timeout: 5000 });
    expect(hooks.attach).toHaveBeenCalled();
    expect(result.current.hostDirs[0]).toMatchObject({ serverId: MEMBER_SERVER, state: "connected" });
  });
});
