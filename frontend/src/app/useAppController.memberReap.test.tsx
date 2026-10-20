import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Hash } from "../ui/icons";
import { useAppController } from "./useAppController";
import type { CentralDirectoryState } from "./useCentralDirectory";
import { bindRemoteSession, installRemoteWorkspace, remoteSessionTransport } from "../lib/remote/remoteWorkspace";
import type { RemoteTransport } from "../lib/remote/remoteTransport";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { RoomDirectoryContinuity } from "./useRoomDirectory";
import type { RoomInviteJoinResponse } from "../lib/roomAdmissionContract";
import type { MemberHostAnchor } from "../lib/central/memberAnchors";
import { roomGuestSessionFromJoinPayload } from "../lib/roomGuestSession";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";

const hooks = vi.hoisted(() => ({
  directory: null as CentralDirectoryState | null,
  refresh: vi.fn(),
  opens: [] as Array<{ roomId: string; auth: unknown; serverSurface: unknown }>,
  closes: [] as Array<{ roomId: string; auth: unknown; serverSurface: unknown }>,
  attach: vi.fn(),
  liveSelect: vi.fn(),
  deadSelect: vi.fn(),
  fetchRoomSettings: vi.fn(),
  initialRooms: [] as RoomDockItem[],
  alert: vi.fn(),
}));

const OWN_MEETING = "own-1";
const MEMBER_MEETING = "hihi-room";
const MEMBER_SERVER = "hihi-server";
const MEMBER_TOKEN = "member-token-1";
const MEMBER_ORIGIN = "https://hihi.test";
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

function centralDirectory(): CentralDirectoryState {
  const server = {
    server_id: MEMBER_SERVER, registration_epoch: "epoch-1", relation: "member" as const,
    alias: "Hihi의 MacBook Air", host_key_fingerprint: "fp", host_public_key_jwk: {},
    endpoint: { origin: MEMBER_ORIGIN, generation: 3, status: "published" as const, mode: "event_secure_v1", protocol: "secure_admission_v1" },
  };
  return {
    status: "connected",
    person: { person_id: "person", display_name: "나", identity_kind: "google" },
    servers: [],
    live: { person: { person_id: "person", display_name: "나", identity_kind: "google" }, servers: [server], server_time: 1 },
  } as unknown as CentralDirectoryState;
}

function memberJoinPayload(): RoomInviteJoinResponse {
  return {
    status: "admitted", request_id: "req-1", session_token: MEMBER_TOKEN, agent_id: "member-agent",
    display_name: "Hihi Member", meeting_id: MEMBER_MEETING, invite_scope: "room",
    participant_type: "human", client_type: "browser", provider_kind: "", connection_kind: "",
    expires_at: "2099-01-01T00:00:00Z", room_label: "Hihi room", room_topic: "",
    room_created_at: "", server_id: MEMBER_SERVER,
    authority_lineage_id: "lineage-hihi", server_product_surface: MEMBER_SURFACE,
    owner_display_name: "", owner_id: "", stable_identity: false, operator: false,
    client_id: "client-test",
    guide: { welcome: "", how_to: [], etiquette: [], session: { expires_in_seconds: 0, rejoin: "" } },
  };
}

function fakeMemberTransport(active: boolean) {
  return {
    hello: { origin: MEMBER_ORIGIN, server_id: MEMBER_SERVER },
    active, onClose: () => () => undefined, close: () => undefined,
    fetch: () => Promise.reject(new Error("no fetch in this test")),
  } as unknown as RemoteTransport;
}

function installMemberSession() {
  const transport = fakeMemberTransport(true);
  const payload = memberJoinPayload();
  const member = { ...roomGuestSessionFromJoinPayload("", payload), centralMember: true as const };
  bindRemoteSession(member.sessionToken, transport, member.serverSurface.server_id);
  installRemoteWorkspace({ transport, owner: null, member, deviceToken: "device-test", clientId: "client-test" });
  return payload;
}

describe("member click after the host reaps the listing channel", () => {
  let attachDeferred: { resolve: (value: unknown) => void } | null = null;

  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    vi.clearAllMocks();
    hooks.opens.length = 0;
    hooks.closes.length = 0;
    attachDeferred = null;
    window.alert = hooks.alert;
    window.localStorage.setItem("agentsassemble.centralSession.v1", JSON.stringify({
      token: "central-token", expires_at: 9_999_999_999, device_id: "device",
      person: { person_id: "person", identity_kind: "google", display_name: "나" },
    }));
    hooks.directory = centralDirectory();
    hooks.initialRooms = [{
      id: "own-room", label: "새 방", meetingId: OWN_MEETING, roomOrigin: "local",
      connectionState: "local", topic: "own", shortLabel: "새", icon: Hash, createdAt: "", tone: "fresh",
    }];
    hooks.fetchRoomSettings.mockImplementation(async (room: RoomDockItem) => ({
      roomId: room.meetingId, label: room.label, topic: room.topic, shortLabel: room.shortLabel,
      appearance: { bannerPreset: "default", notifications: "default", inviteScope: "room" },
      channelSettings: {}, conversationMode: "ordered", toolMode: "chat", orderedExcludePreviousSpeaker: false,
    }));
    // The startup background attach listed rooms on an admit-only channel;
    // the host has since reaped it (transport dead): any select on it fails
    // with a bare network error, no HTTP status.
    hooks.deadSelect.mockRejectedValue(new Error("연결이 끝났어요. 서버 목록에서 다시 연결해 주세요."));
    hooks.liveSelect.mockImplementation(async () => installMemberSession());
    const anchorFor = async () => {
      const anchor: MemberHostAnchor = {
        server_id: MEMBER_SERVER, registration_epoch: "epoch-1", host_key_fingerprint: "fp",
        endpoint_origin: MEMBER_ORIGIN, endpoint_generation: 3,
        rooms: [{ room_id: MEMBER_MEETING, name: "Hihi room" }], hidden: false, updated_at: Date.now(),
      };
      const { saveMemberAnchor } = await import("../lib/central/memberAnchors");
      saveMemberAnchor("person", anchor);
      return anchor;
    };
    hooks.attach.mockImplementationOnce(async () => ({
      admission: { transport: fakeMemberTransport(false), close: () => undefined, select: (...args: unknown[]) => hooks.deadSelect(...args) },
      rooms: [{ room_id: MEMBER_MEETING, name: "Hihi room" }],
      anchor: await anchorFor(),
    }));
    hooks.attach.mockImplementation(async () => new Promise((resolve) => { attachDeferred = { resolve }; }).then(async () => ({
      admission: { transport: fakeMemberTransport(true), close: () => undefined, select: (...args: unknown[]) => hooks.liveSelect(...args) },
      rooms: [{ room_id: MEMBER_MEETING, name: "Hihi room" }],
      anchor: await anchorFor(),
    })));
  });

  afterEach(cleanup);

  it("replays startup attach, reap, click, re-attach, admission complete", async () => {
    const { result } = renderHook(() => useAppController("device-test", "client-test"));
    await waitFor(() => {
      const rooms = result.current.rooms;
      if (rooms.length !== 2) throw new Error(`rail not merged yet: ${rooms.map((room) => room.label).join(",")}`);
    });
    const memberId = result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING)!.id;
    await waitFor(() => expect(result.current.activeRoom.meetingId).toBe(OWN_MEETING));
    expect(hooks.opens.filter((open) => open.roomId === OWN_MEETING)).toHaveLength(1);

    let clicked: Promise<void> | null = null;
    await act(async () => {
      clicked = result.current.selectRoom(memberId);
      await Promise.resolve();
    });
    // While the re-attach is in flight the clicked member room stays in the
    // rail (connecting on its last-known rooms), never vanishing.
    await waitFor(() => expect(attachDeferred).not.toBeNull());
    await act(async () => { await Promise.resolve(); });
    expect(result.current.rooms.some((room) => room.id === memberId)).toBe(true);
    const midFlight = result.current.rooms.find((room) => room.id === memberId)!;
    expect(midFlight.connectionState).not.toBe("disconnected");
    const opensBeforeAdmission = hooks.opens.length;
    const closesBeforeAdmission = hooks.closes.length;

    await act(async () => {
      attachDeferred!.resolve(undefined);
      await clicked!;
    });

    // The click attaches fresh without a doomed first select on the reaped
    // channel, then selects exactly once.
    expect(hooks.deadSelect).not.toHaveBeenCalled();
    expect(hooks.liveSelect).toHaveBeenCalledTimes(1);
    expect(result.current.memberSelectError).toBeNull();
    // Admission complete: the clicked member room is active and its socket
    // opens with its own session; the own socket is closed exactly once by
    // the room switch and never reopened on the own room.
    await waitFor(() => expect(result.current.activeRoom.meetingId).toBe(MEMBER_MEETING));
    expect(result.current.activeRoom.id).toBe(memberId);
    const memberOpen = hooks.opens.find((open) => open.roomId === MEMBER_MEETING);
    expect(memberOpen).toMatchObject({ auth: { kind: "session", sessionToken: MEMBER_TOKEN } });
    expect(memberOpen!.serverSurface).toBe(MEMBER_SURFACE);
    expect(remoteSessionTransport(MEMBER_TOKEN)).toBeTruthy();
    expect(hooks.opens.filter((open) => open.roomId === OWN_MEETING)).toHaveLength(1);
    expect(hooks.closes.filter((close) => close.roomId === OWN_MEETING)).toHaveLength(1);
    expect(opensBeforeAdmission).toBe(1);
    expect(closesBeforeAdmission).toBe(0);
  });
});
