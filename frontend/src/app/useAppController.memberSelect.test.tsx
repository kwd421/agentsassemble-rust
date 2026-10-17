import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Hash } from "lucide-react";
import { useAppController } from "./useAppController";
import type { CentralDirectoryState } from "./useCentralDirectory";
import { bindRemoteSession, installRemoteWorkspace, remoteSessionTransport, remoteWorkspaceFor } from "../lib/remote/remoteWorkspace";
import type { RemoteTransport } from "../lib/remote/remoteTransport";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { RoomDirectoryContinuity } from "./useRoomDirectory";
import type { RoomInviteJoinResponse } from "../lib/roomAdmissionContract";
import type { MemberHostAnchor } from "../lib/central/memberAnchors";
import { roomGuestSessionFromJoinPayload } from "../lib/roomGuestSession";
import type { RoomAgentSession, RoomMember } from "../api";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";

const hooks = vi.hoisted(() => ({
  directory: null as CentralDirectoryState | null,
  refresh: vi.fn(),
  canonicalCalls: [] as Array<{ roomId: string; auth: unknown; viewer: string }>,
  attach: vi.fn(),
  select: vi.fn(),
  fetchRoomSettings: vi.fn(),
  initialRooms: [] as RoomDockItem[],
}));

const OWN_MEETING = "own-1";
const MEMBER_MEETING = "hihi-room";
const MEMBER_SERVER = "hihi-server";
const MEMBER_TOKEN = "member-token-1";

function participant(roomId: string, id: string, type: string, name: string): RoomMember {
  return {
    room_id: roomId, participant_id: id, display_name: name, avatar_image_url: "",
    participant_type: type, status: "joined", role: type === "agent" ? "agent" : "human",
    owner_id: "owner-1", muted: false, created_at: "", updated_at: "",
  };
}

function agentSession(roomId: string, sessionId: string, participantId: string, name: string): RoomAgentSession {
  return {
    avatar_image_url: "", room_id: roomId, session_id: sessionId, participant_id: participantId,
    display_name: name, status: "attached", runtime_status: "idle", enabled: true,
    provider_kind: "codex", runtime_kind: "managed", connection_kind: "managed",
    external_owned: false, process_ownership: "host", model: "model", reasoning_effort: "",
    service_tier: "", variant: "", execution_harness: "", permission_mode: "",
    max_output_tokens: 0, catalog_revision: "rev", persona_card_id: "", persona_card: null,
    transport: "", last_seen_event_id: "", last_seen_seq: 0, last_provider_sync_event_id: "",
    last_provider_sync_seq: 0, bootstrap_cutoff_seq: 0, turn_count: 0, active_turn_id: "",
    turn_phase: "", last_error: "", last_error_code: "", recovery_required: false,
    provider_session_active: false, provider_session_reused: false, created_at: "", updated_at: "",
  };
}

const fixtures = vi.hoisted(() => ({
  own: {
    channels: [
      { id: "c-ilban", name: "일반", type: "text", position: 0, created_at: "" },
      { id: "c-plan", name: "기획", type: "text", position: 1, created_at: "" },
    ],
    agentName: "Own Agent",
  },
  member: {
    channels: [{ id: "c-notice", name: "Hihi 공지", type: "text", position: 0, created_at: "" }],
    agentName: "Hihi Agent",
  },
}));

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

vi.mock("../useCanonicalRoom", () => ({
  useCanonicalRoom: (options: { roomId: string; auth?: unknown; viewerParticipantId?: string }) => {
    hooks.canonicalCalls.push({ roomId: options.roomId, auth: options.auth, viewer: options.viewerParticipantId ?? "" });
    const auth = options.auth as { kind?: string; sessionToken?: string; meetingId?: string } | undefined;
    // Each host only serves its own room: a foreign bearer never loads.
    const served =
      (options.roomId === OWN_MEETING && auth?.kind === "host" && auth.meetingId === OWN_MEETING) ||
      (options.roomId === MEMBER_MEETING && auth?.kind === "session" && auth.sessionToken === MEMBER_TOKEN);
    const owned = options.roomId === OWN_MEETING;
    const channels = served ? (owned ? fixtures.own.channels : fixtures.member.channels) : [];
    const agentName = owned ? fixtures.own.agentName : fixtures.member.agentName;
    const meeting = options.roomId;
    const roomUid = owned ? "own-uid-1" : "hihi-uid-1";
    return {
      room: served ? { room_id: meeting, room_uid: roomUid, label: "", status: "active", created_at: "", updated_at: "" } : null,
      socket: {
        ready: () => served,
        resync: () => undefined, close: () => undefined,
        command: async () => ({ result: {} }),
        historyBefore: async () => ({ room_id: meeting, channel_id: "lobby", events: [], oldest_seq: 0, last_seq: 0, has_more_before: false }),
      },
      connectionState: served ? "connected" : "connecting",
      lastError: null, syncIssue: null, events: [], channelMessageCursors: {}, timelineEvents: [],
      displayResourceBase: "",
      participants: served ? [participant(meeting, owned ? "own-agent" : "hihi-agent", "agent", agentName)] : [],
      participantRecords: served ? [participant(meeting, owned ? "own-agent" : "hihi-agent", "agent", agentName)] : [],
      participantProfiles: {},
      roomSettings: served ? {
        roomId: meeting, revision: "rev-1", label: "", topic: "", shortLabel: "",
        appearance: {}, conversationMode: "ordered", toolMode: "chat",
        orderedExcludePreviousSpeaker: false, channels,
      } : null,
      agentSessions: served ? [agentSession(meeting, owned ? "sess-own" : "sess-hihi", owned ? "own-agent" : "hihi-agent", agentName)] : [],
      providerRequests: [],
      capabilities: served ? (owned
        ? { "room.manage": true, "agent.control": true, "message.send": true, "message.modify": true }
        : { "message.send": true, "message.modify": true }) : {},
      availableProviders: [], providerCatalog: { catalog_revision: "rev", providers: [] },
      agentSessionProgress: null, pluginEnvelopes: [],
      history: { initialized: served, oldestSeq: served ? 1 : 0, lastSeq: served ? 5 : 0, hasMoreBefore: false, windowRevision: 1 },
      loadHistory: async () => ({ loadedCount: 0, oldestSeq: 0, hasMoreBefore: false, events: [] }),
      sendAgentControl: async () => undefined, sendAgentConfigure: async () => undefined,
      sendAgentProfileUpdate: async () => undefined, sendParticipantMute: async () => undefined,
      sendParticipantRemove: async () => undefined, sendParticipantRole: async () => undefined,
      sendRoomSettingsUpdate: async (updates: unknown) => updates,
    };
  },
}));

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
    endpoint: { origin: "https://hihi.test", generation: 3, status: "published" as const, mode: "event_secure_v1", protocol: "secure_admission_v1" },
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
    authority_lineage_id: "lineage-hihi", server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
    owner_display_name: "", owner_id: "", stable_identity: false, operator: false,
    client_id: "client-test",
    guide: { welcome: "", how_to: [], etiquette: [], session: { expires_in_seconds: 0, rejoin: "" } },
  };
}

describe("member room selection on an owner local host", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.sessionStorage.clear();
    vi.clearAllMocks();
    hooks.canonicalCalls.length = 0;
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
    const transport = { hello: { origin: "https://hihi.test" }, active: true, onClose: () => undefined, close: () => undefined };
    hooks.select.mockImplementation(async () => {
      // Production SecureMemberAdmission.install binds the session AND
      // installs the member workspace on the same transport. The install
      // changes default-origin routing and the session display origin, so
      // the test must do it too instead of binding the session alone.
      const payload = memberJoinPayload();
      const member = { ...roomGuestSessionFromJoinPayload("", payload), centralMember: true as const };
      bindRemoteSession(member.sessionToken, transport as unknown as RemoteTransport, member.serverSurface.server_id);
      installRemoteWorkspace({ transport: transport as unknown as RemoteTransport, owner: null, member, deviceToken: "device-test", clientId: "client-test" });
      return payload;
    });
    hooks.attach.mockImplementation(async () => {
      const anchor: MemberHostAnchor = {
        server_id: MEMBER_SERVER, registration_epoch: "epoch-1", host_key_fingerprint: "fp",
        endpoint_origin: "https://hihi.test", endpoint_generation: 3,
        rooms: [{ room_id: MEMBER_MEETING, name: "Hihi room" }], hidden: false, updated_at: Date.now(),
      };
      const { saveMemberAnchor } = await import("../lib/central/memberAnchors");
      saveMemberAnchor("person", anchor);
      return {
        admission: { transport: { onClose: () => undefined }, close: () => undefined, select: (...args: unknown[]) => hooks.select(...args) },
        rooms: [{ room_id: MEMBER_MEETING, name: "Hihi room" }],
        anchor,
      };
    });
  });

  afterEach(cleanup);

  it("loads the member room in place and keeps switching between both hosts", async () => {
    const { result } = renderHook(() => useAppController("device-test", "client-test"));
    await waitFor(() => {
      const rooms = result.current.rooms;
      if (rooms.length !== 2) throw new Error(`rail not merged yet: ${rooms.map((room) => room.label).join(",")}`);
    });
    const ownId = result.current.rooms.find((room) => room.meetingId === OWN_MEETING)!.id;
    const memberId = result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING)!.id;
    const memberEntry = result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING)!;
    expect(memberEntry).toMatchObject({ roomOrigin: "remote_server", serverId: MEMBER_SERVER, connectionState: "connected" });

    const lastAuth = () => hooks.canonicalCalls[hooks.canonicalCalls.length - 1];
    const channelLabels = () => result.current.visibleChannels.map((channel) => channel.label);
    const agentNames = () => result.current.scopedAgents.map((agent) => agent.display_name);

    // Own room first: host authority, own channels and agents.
    await waitFor(() => expect(result.current.activeRoom.meetingId).toBe(OWN_MEETING));
    await waitFor(() => expect(lastAuth()).toMatchObject({ roomId: OWN_MEETING, auth: { kind: "host", meetingId: OWN_MEETING } }));
    await waitFor(() => expect(channelLabels()).toEqual(expect.arrayContaining(["일반", "기획"])));
    expect(agentNames()).toContain("Own Agent");
    expect(result.current.guestLocked).toBe(false);

    // Select the member room: its host session loads its own channels/agents in place.
    await act(async () => { await result.current.selectRoom(memberId); });
    await waitFor(() => expect(result.current.activeRoom.meetingId).toBe(MEMBER_MEETING));
    expect(result.current.activeRoom.id).toBe(memberId);
    await waitFor(() => expect(lastAuth()).toMatchObject({
      roomId: MEMBER_MEETING, auth: { kind: "session", sessionToken: MEMBER_TOKEN },
    }));
    await waitFor(() => expect(channelLabels()).toContain("Hihi 공지"));
    expect(channelLabels()).not.toContain("기획");
    expect(agentNames()).toContain("Hihi Agent");
    expect(result.current.guestLocked).toBe(true);

    // Back to the own room: host authority and own channels/agents return.
    await act(async () => { await result.current.selectRoom(ownId); });
    await waitFor(() => expect(result.current.activeRoom.meetingId).toBe(OWN_MEETING));
    await waitFor(() => expect(lastAuth()).toMatchObject({ roomId: OWN_MEETING, auth: { kind: "host", meetingId: OWN_MEETING } }));
    await waitFor(() => expect(channelLabels()).toEqual(expect.arrayContaining(["일반", "기획"])));
    expect(agentNames()).toContain("Own Agent");
    expect(result.current.guestLocked).toBe(false);

    // And back to the member room: still live without another admission.
    expect(result.current.guestLocked).toBe(false);

    // And back to the member room: still live without another admission.
    await act(async () => { await result.current.selectRoom(memberId); });
    await waitFor(() => expect(lastAuth()).toMatchObject({
      roomId: MEMBER_MEETING, auth: { kind: "session", sessionToken: MEMBER_TOKEN },
    }));
    await waitFor(() => expect(channelLabels()).toContain("Hihi 공지"));
    expect(agentNames()).toContain("Hihi Agent");
    expect(hooks.select).toHaveBeenCalledTimes(1);

    // The rail keeps both hosts and selection never touches central.
    expect(result.current.rooms).toHaveLength(2);
    expect(result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING))
      .toMatchObject({ connectionState: "connected" });
    expect(result.current.activeRoom).toMatchObject({ serverId: MEMBER_SERVER, meetingId: MEMBER_MEETING });
    expect(hooks.refresh).not.toHaveBeenCalled();
  });

  it("surfaces the inline Korean failure for a stale rail click instead of silence", async () => {
    const alert = vi.spyOn(window, "alert").mockImplementation(() => undefined);
    try {
      const { result } = renderHook(() => useAppController("device-test", "client-test"));
      await waitFor(() => {
        if (result.current.rooms.length !== 2) throw new Error("rail not merged yet");
      });
      await act(async () => { await result.current.selectRoom("stale-room-id"); });
      expect(result.current.memberSelectError).toEqual({
        roomId: "stale-room-id", message: "방을 열지 못했어요. 다시 시도해 주세요.",
      });
      expect(alert).not.toHaveBeenCalled();
    } finally {
      alert.mockRestore();
    }
  });

  it("stays on the clicked member room with the inline failure when the admission is superseded", async () => {
    const { result } = renderHook(() => useAppController("device-test", "client-test"));
    await waitFor(() => {
      if (result.current.rooms.length !== 2) throw new Error("rail not merged yet");
    });
    const memberId = result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING)!.id;
    const { verifyAndBindRoomSessionSurface } = await import("../lib/roomDirectoryContract");
    vi.mocked(verifyAndBindRoomSessionSurface).mockResolvedValueOnce(false);
    await act(async () => { await result.current.selectRoom(memberId); });
    // Silence is the defect: the selection still moves to the clicked room
    // and carries the short Korean message instead of abandoning the tap.
    expect(result.current.activeRoom.meetingId).toBe(MEMBER_MEETING);
    expect(result.current.memberSelectError).toEqual({
      roomId: memberId, message: "방을 열지 못했어요. 다시 시도해 주세요.",
    });
  });

  it("moves the selection to the member room with an inline Korean message when select fails", async () => {
    const alert = vi.spyOn(window, "alert").mockImplementation(() => undefined);
    try {
      const { result } = renderHook(() => useAppController("device-test", "client-test"));
      await waitFor(() => {
        if (result.current.rooms.length !== 2) throw new Error("rail not merged yet");
      });
      const memberId = result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING)!.id;
      const successSelect = hooks.select.getMockImplementation();
      if (!successSelect) throw new Error("member select mock missing");
      // Both the first select and the single fresh-challenge retry fail.
      hooks.select.mockRejectedValue(new Error("연결이 끝났어요."));
      await act(async () => { await result.current.selectRoom(memberId); });
      expect(hooks.select).toHaveBeenCalledTimes(2);
      // Silence is the defect: the rail selection reflects the shown room and
      // the room carries the short Korean message instead of an alert.
      expect(result.current.activeRoom.meetingId).toBe(MEMBER_MEETING);
      expect(result.current.memberSelectError).toEqual({
        roomId: memberId, message: "방을 열지 못했어요. 다시 시도해 주세요.",
      });
      expect(alert).not.toHaveBeenCalled();
      // The next tap retries through a fresh admission and clears the message.
      hooks.select.mockImplementation(successSelect);
      await act(async () => { await result.current.selectRoom(memberId); });
      await waitFor(() => expect(result.current.memberSelectError).toBeNull());
      expect(result.current.activeRoom.meetingId).toBe(MEMBER_MEETING);
    } finally {
      alert.mockRestore();
    }
  });

  it("never drives a member room with host auth when the rail menu opens it", async () => {
    const { result } = renderHook(() => useAppController("device-test", "client-test"));
    await waitFor(() => {
      if (result.current.rooms.length !== 2) throw new Error("rail not merged yet");
    });
    const memberEntry = result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING)!;
    await act(async () => {
      result.current.openRoomMenu(
        { preventDefault: () => undefined, stopPropagation: () => undefined, clientX: 10, clientY: 10 } as unknown as Parameters<typeof result.current.openRoomMenu>[0],
        memberEntry,
      );
    });
    // The menu opens without making the member room active: activation needs
    // its own room session through the rail tap admission, never this
    // computer's host authority.
    expect(result.current.roomMenu?.roomId).toBe(memberEntry.id);
    expect(result.current.activeRoom.meetingId).toBe(OWN_MEETING);
    expect(hooks.select).not.toHaveBeenCalled();
    for (const call of hooks.canonicalCalls) {
      const auth = call.auth as { kind?: string; meetingId?: string } | undefined;
      expect(auth?.kind === "host" && auth.meetingId === MEMBER_MEETING).toBe(false);
    }
  });

  it("routes invite through member admission instead of host auth", async () => {
    const { result } = renderHook(() => useAppController("device-test", "client-test"));
    await waitFor(() => {
      if (result.current.rooms.length !== 2) throw new Error("rail not merged yet");
    });
    const memberId = result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING)!.id;
    await act(async () => { await result.current.inviteRoom(memberId); });
    expect(hooks.select).toHaveBeenCalled();
    await waitFor(() => expect(result.current.activeRoom.meetingId).toBe(MEMBER_MEETING));
    expect(result.current.inviteModalRoom?.id).toBe(memberId);
    await waitFor(() => expect(hooks.canonicalCalls.at(-1)).toMatchObject({
      roomId: MEMBER_MEETING, auth: { kind: "session", sessionToken: MEMBER_TOKEN },
    }));
    for (const call of hooks.canonicalCalls) {
      const auth = call.auth as { kind?: string; meetingId?: string } | undefined;
      expect(auth?.kind === "host" && auth.meetingId === MEMBER_MEETING).toBe(false);
    }
  });

  it("routes settings through member admission instead of host auth", async () => {
    const { result } = renderHook(() => useAppController("device-test", "client-test"));
    await waitFor(() => {
      if (result.current.rooms.length !== 2) throw new Error("rail not merged yet");
    });
    const memberId = result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING)!.id;
    await act(async () => { await result.current.openRoomSettings(memberId); });
    expect(hooks.select).toHaveBeenCalled();
    await waitFor(() => expect(result.current.activeRoom.meetingId).toBe(MEMBER_MEETING));
    await waitFor(() => expect(hooks.canonicalCalls.at(-1)).toMatchObject({
      roomId: MEMBER_MEETING, auth: { kind: "session", sessionToken: MEMBER_TOKEN },
    }));
    for (const call of hooks.canonicalCalls) {
      const auth = call.auth as { kind?: string; meetingId?: string } | undefined;
      expect(auth?.kind === "host" && auth.meetingId === MEMBER_MEETING).toBe(false);
    }
  });

  it("closes the member transport, clears its session and drops its rooms when central confirms the membership is gone", async () => {
    const attachTransportClose = vi.fn();
    const workspaceTransportClose = vi.fn();
    hooks.initialRooms = [
      {
        id: "own-room", label: "새 방", meetingId: OWN_MEETING, roomOrigin: "local",
        connectionState: "local", topic: "own", shortLabel: "새", icon: Hash, createdAt: "", tone: "fresh",
      },
      {
        id: "server-hihi-server-hihi-room", label: "Hihi room", meetingId: MEMBER_MEETING,
        roomOrigin: "remote_server", serverId: MEMBER_SERVER, connectionState: "disconnected",
        topic: "Hihi room", shortLabel: "H", icon: Hash, createdAt: "", tone: "resident",
      },
    ];
    const workspaceTransport = { hello: { origin: "https://hihi.test" }, active: true, onClose: () => undefined, close: workspaceTransportClose };
    hooks.select.mockImplementation(async () => {
      const payload = memberJoinPayload();
      const member = { ...roomGuestSessionFromJoinPayload("", payload), centralMember: true as const };
      bindRemoteSession(member.sessionToken, workspaceTransport as unknown as RemoteTransport, member.serverSurface.server_id);
      installRemoteWorkspace({ transport: workspaceTransport as unknown as RemoteTransport, owner: null, member, deviceToken: "device-test", clientId: "client-test" });
      return payload;
    });
    hooks.attach.mockImplementation(async () => {
      const anchor: MemberHostAnchor = {
        server_id: MEMBER_SERVER, registration_epoch: "epoch-1", host_key_fingerprint: "fp",
        endpoint_origin: "https://hihi.test", endpoint_generation: 3,
        rooms: [{ room_id: MEMBER_MEETING, name: "Hihi room" }], hidden: false, updated_at: Date.now(),
      };
      const { saveMemberAnchor } = await import("../lib/central/memberAnchors");
      saveMemberAnchor("person", anchor);
      return {
        admission: { transport: { onClose: () => undefined, close: attachTransportClose, active: true }, close: () => undefined, select: (...args: unknown[]) => hooks.select(...args) },
        rooms: [{ room_id: MEMBER_MEETING, name: "Hihi room" }],
        anchor,
      };
    });
    const { result, rerender } = renderHook(() => useAppController("device-test", "client-test"));
    await waitFor(() => {
      if (result.current.rooms.length !== 2) throw new Error("rail not merged yet");
    });
    const memberId = result.current.rooms.find((room) => room.meetingId === MEMBER_MEETING)!.id;
    await act(async () => { await result.current.selectRoom(memberId); });
    await waitFor(() => expect(result.current.activeRoom.meetingId).toBe(MEMBER_MEETING));
    await waitFor(() => expect(hooks.canonicalCalls.at(-1)).toMatchObject({
      roomId: MEMBER_MEETING, auth: { kind: "session", sessionToken: MEMBER_TOKEN },
    }));
    expect(attachTransportClose).not.toHaveBeenCalled();
    expect(workspaceTransportClose).not.toHaveBeenCalled();
    expect(remoteWorkspaceFor(MEMBER_SERVER)).not.toBeNull();
    expect(remoteSessionTransport(MEMBER_TOKEN)).toBeTruthy();
    // The host kicks/bans the member (or central revokes it): while still
    // connected, central no longer lists the membership. A temporary central
    // outage keeps every anchor; only this connected confirmation is loss.
    await act(async () => {
      const live = hooks.directory?.live;
      hooks.directory = {
        status: "connected",
        person: { person_id: "person", display_name: "나", identity_kind: "google" },
        servers: [],
        live: { person: live!.person, servers: [], server_time: 2 },
      } as unknown as CentralDirectoryState;
      rerender();
    });
    // The adopted channel and the room bearer close instead of staying open,
    // the kicked rooms disappear from the rail (never linger disconnected),
    // and the selection falls back to the own room.
    await waitFor(() => expect(
      result.current.rooms.some((room) => room.meetingId === MEMBER_MEETING)
    ).toBe(false));
    expect(attachTransportClose).toHaveBeenCalled();
    expect(workspaceTransportClose).toHaveBeenCalled();
    expect(remoteWorkspaceFor(MEMBER_SERVER)).toBeNull();
    expect(remoteSessionTransport(MEMBER_TOKEN)).toBeUndefined();
    expect(result.current.guestSession).toBeNull();
    expect(result.current.admittedSessionToken).toBe("");
    expect(result.current.activeRoom.meetingId).toBe(OWN_MEETING);
    expect(hooks.attach).toHaveBeenCalledTimes(1);
  });
});
