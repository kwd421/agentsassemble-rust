import { beforeEach, describe, expect, it } from "vitest";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";
import { persistRoomGuestSession, type RoomGuestSession } from "./roomGuestSession";
import { consumeOperatorPairingTokenFromUrl } from "./roomGuestSession";
import {
  createStartupRoute,
  createFreshRoom, roomIsDisconnected,
  mergeHostRoomsIntoDock,
  mergeServerRoomsIntoDock,
  roomDockIdentity,
  type RoomDockItem,
} from "./roomDockModel";

const SESSION: RoomGuestSession = {
  inviteToken: "older-invite",
  sessionToken: "session-1",
  meetingId: "room-1",
  agentId: "guest-1",
  displayName: "Guest",
  inviteScope: "room",
  expiresAt: "2099-01-01T00:00:00Z",
  joinedAt: "2026-07-15T00:00:00Z",
  operator: false,
  roomLabel: "Room One",
  serverSurface: {
    server_id: "11111111-1111-4111-8111-111111111111",
    authority_lineage_id: "22222222-2222-4222-8222-222222222222",
    server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
  },
};

describe("createStartupRoute", () => {
  beforeEach(() => {
    window.localStorage.clear();
    window.history.replaceState({}, "", "/");
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  });

  it("does not fabricate a desktop general room before server hydration", () => {
    Object.assign(window, { __TAURI_INTERNALS__: { invoke: () => Promise.resolve() } });

    const route = createStartupRoute();

    expect(route.startupRooms).toEqual([]);
    expect(route.activeRoomId).toBe("");
  });

  it("does not activate a deferred game from startup parameters", () => {
    window.history.replaceState({}, "", "/?mafia=deferred-game&mafiaGameId=other-game");
    const route = createStartupRoute();
    expect(route.initialChannel).toBe("lobby");
    expect(route.directRoom).toBeNull();
    expect(route.startupRooms).toEqual([]);
    expect(route.activeRoomId).toBe("");
  });

  it("keeps a stored room session while a new invite is preflighted", () => {
    persistRoomGuestSession(SESSION);
    window.history.replaceState({}, "", "/join?token=new-invite");

    const route = createStartupRoute();

    expect(route.guestJoinToken).toBe("new-invite");
    expect(route.guestSession).toEqual(expect.objectContaining(SESSION));
    expect(route.guestInvite?.meetingId).toBe("pending-join");
  });

  it("does not treat a Room Connector invite as a human guest join", () => {
    window.history.replaceState({}, "", "/join?token=aaci1.connector-secret");
    const route = createStartupRoute();
    expect(route.guestJoinToken).toBe("");
    expect(route.connectorJoinUrl).toBe(`${window.location.origin}/join?token=aaci1.connector-secret`);
    expect(route.guestInvite?.meetingId).toBe("pending-join");
  });

  it("does not let legacy query state steer a canonical invite", () => {
    window.history.replaceState(
      {},
      "",
      "/join?token=new-invite&guest=1&room=legacy-room&scope=read_only"
    );

    const route = createStartupRoute();

    expect(route.guestJoinToken).toBe("new-invite");
    expect(route.guestInvite?.meetingId).toBe("pending-join");
    expect(route.guestInvite?.inviteScope).toBe("room");
  });

  it("captures and immediately removes a one-time pairing token from the URL", () => {
    window.history.replaceState({}, "", "/pair?token=aap1_secret-token");

    const pairingToken = consumeOperatorPairingTokenFromUrl();
    const route = createStartupRoute({ operatorPairingPending: Boolean(pairingToken) });

    expect(pairingToken).toBe("aap1_secret-token");
    expect(route.guestJoinToken).toBe("");
    expect(route.guestInvite?.meetingId).toBe("pending-pairing");
    expect(window.location.pathname).toBe("/pair");
    expect(window.location.search).toBe("");
  });
});

describe("durable room identity", () => {
  it("preserves a directory-owned canonical room identifier unchanged", () => {
    const exactRoomId = "\ufeffroom";
    const [room] = mergeServerRoomsIntoDock(
      [],
      [{ room_id: exactRoomId, room_uid: "room-uid", label: "Exact" }],
      window.location.origin,
      "server-id"
    );

    expect(room.meetingId).toBe(exactRoomId);
  });

  it("replaces a stale cached alias with the verified directory room identifier", () => {
    const exactRoomId = "\ufeffroom";
    const [canonical] = mergeServerRoomsIntoDock(
      [],
      [{ room_id: exactRoomId, room_uid: "room-uid", label: "Exact" }],
      window.location.origin,
      "server-id"
    );

    const [reconciled] = mergeServerRoomsIntoDock(
      [{ ...canonical, meetingId: "room" }],
      [{ room_id: exactRoomId, room_uid: "room-uid", label: "Exact" }],
      window.location.origin,
      "server-id"
    );

    expect(reconciled.meetingId).toBe(exactRoomId);
  });

  it("does not collapse equal room aliases owned by different servers", () => {
    const first = mergeServerRoomsIntoDock(
      [],
      [{ room_id: "general", room_uid: "room-uid-a", label: "First" }],
      "https://first.example",
      "server-a"
    );
    const second = mergeServerRoomsIntoDock(
      first,
      [{ room_id: "general", room_uid: "room-uid-b", label: "Second" }],
      "https://second.example",
      "server-b"
    );

    expect(second).toHaveLength(2);
    expect(new Set(second.map(roomDockIdentity))).toEqual(
      new Set(["server-a:room-uid-a", "server-b:room-uid-b"])
    );
  });
});

it("marks a disconnected local runtime room as disconnected too", () => {
  expect(roomIsDisconnected({ ...createFreshRoom(), connectionState: "disconnected" })).toBe(true);
});

describe("multihost merge", () => {
  const hostRoom = (server: string, meeting: string, state: RoomDockItem["connectionState"] = "connected"): RoomDockItem => ({
    ...createFreshRoom(),
    id: `server-${server}-${meeting}`,
    label: `${server} ${meeting}`,
    meetingId: meeting,
    roomUid: `${meeting}-uid`,
    serverId: server,
    roomOrigin: "remote_server",
    serverOrigin: `https://${server}.test`,
    connectionState: state,
  });
  const directoryRoom = (meeting: string) => ({ room_id: meeting, room_uid: `${meeting}-uid`, label: meeting });

  it("keeps host B live while host A drops, and only A's rooms go disconnected", () => {
    const localRoom = { ...createFreshRoom(), meetingId: "local-room" };
    const merged = mergeHostRoomsIntoDock(
      [hostRoom("server-a", "general", "connected"), hostRoom("server-b", "general", "connected"), localRoom],
      [
        { serverId: "local", serverOrigin: window.location.origin, rooms: [directoryRoom("local-room")], state: "connected", local: true },
        { serverId: "server-a", serverOrigin: "https://server-a.test", rooms: null, state: "disconnected" },
        { serverId: "server-b", serverOrigin: "https://server-b.test", rooms: [directoryRoom("general")], state: "connected" },
      ]
    );
    expect(merged.map((room) => room.connectionState)).toEqual(["disconnected", "connected", "local"]);
    expect(merged).toHaveLength(3);
  });

  it("drops a room the live host no longer lists, without a tombstone", () => {
    const merged = mergeHostRoomsIntoDock(
      [hostRoom("server-a", "general", "connected"), hostRoom("server-a", "removed", "connected")],
      [{ serverId: "server-a", serverOrigin: "https://server-a.test", rooms: [directoryRoom("general")], state: "connected" }]
    );
    expect(merged.map((room) => room.meetingId)).toEqual(["general"]);
  });

  it("preserves a live peer's rooms through a single-host merge while unknown hosts go disconnected", () => {
    const peer = hostRoom("server-peer", "general", "connected");
    const stranger = hostRoom("server-gone", "general", "connected");
    const merged = mergeServerRoomsIntoDock(
      [peer, stranger],
      [directoryRoom("local-room")],
      window.location.origin,
      "local-server",
      { local: true, peers: [{ serverId: "server-peer" }] }
    );
    expect(merged.find((room) => room.serverId === "server-peer")).toMatchObject({
      connectionState: "connected",
      label: "server-peer general",
    });
    expect(merged.find((room) => room.serverId === "server-gone")?.connectionState).toBe("disconnected");
    expect(merged.some((room) => room.meetingId === "local-room")).toBe(true);
  });
});
