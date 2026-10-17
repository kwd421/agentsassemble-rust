import { afterEach, describe, expect, it, vi } from "vitest";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";
import { roomDockIdentity, roomFromGuestSession } from "./roomDockModel";
import {
  loadRoomGuestSession,
  persistRoomGuestSession,
  roomGuestSessionExpired,
  roomGuestSessionFromJoinPayload,
  roomGuestSessionFromPairingPayload,
} from "./roomGuestSession";
import {
  closeRemoteWorkspace,
  installRemoteWorkspace,
} from "./remote/remoteWorkspace";
import type { RemoteTransport } from "./remote/remoteTransport";

import { parseOperatorPairingRedeemResponse } from "./roomAdmissionContract";

describe("guest room projection", () => {
  const serverSurface = {
    server_id: "11111111-1111-4111-8111-111111111111",
    authority_lineage_id: "22222222-2222-4222-8222-222222222222",
    server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
  };
  const joinResponse = {
    ...serverSurface,
    status: "admitted" as const,
    request_id: "123e4567-e89b-42d3-a456-426614174000",
    session_token: "session-token",
    meeting_id: "room-1",
    agent_id: "guest-1",
    display_name: "Guest",
    invite_scope: "room" as const,
    participant_type: "human" as const,
    client_type: "browser" as const,
    provider_kind: "manual",
    owner_display_name: "",
    owner_id: "owner-1",
    stable_identity: false,
    operator: false,
    connection_kind: "native_remote_room_client",
    client_id: "client-1",
    expires_at: "2026-07-12T00:00:00Z",
    room_label: "Night Council",
    room_topic: "Old elevator",
    room_created_at: "2026-07-10T00:00:00Z",
    guide: {
      welcome: "Welcome",
      how_to: ["Read and send messages."],
      etiquette: [],
      session: { expires_in_seconds: 3600, rejoin: "Request another invite." },
    },
  };

  it("keeps central member sessions memory-only so restart re-admits from the anchor", () => {
    const session = { ...roomGuestSessionFromJoinPayload("", joinResponse), centralMember: true };
    persistRoomGuestSession(session);
    expect(loadRoomGuestSession()).toBeNull();
    expect(window.localStorage.getItem("agentsassemble.roomGuestSession.v1")).toBeNull();
    persistRoomGuestSession(null);
  });

  it("restores native pairing with no fixed expiry after browser reopening", () => {
    const { request_id: _request, guide: _guide, client_id: _client, owner_display_name: _ownerName, ...common } = joinResponse;
    const admitted = parseOperatorPairingRedeemResponse({ ...common, operator: true, stable_identity: true, expires_at: null });
    const session = roomGuestSessionFromPairingPayload(admitted);
    persistRoomGuestSession(session);
    expect(loadRoomGuestSession()).toEqual(session);
    expect(roomGuestSessionExpired(loadRoomGuestSession(), Date.now() + 86_400_000)).toBe(false);
    persistRoomGuestSession(null);
  });

  it("lets the host revalidate a stored operator deadline after upgrade", () => {
    const session = { ...roomGuestSessionFromJoinPayload("", joinResponse), operator: true };
    persistRoomGuestSession(session);
    expect(roomGuestSessionExpired(loadRoomGuestSession(), Date.parse(joinResponse.expires_at) + 1)).toBe(false);
    expect(roomGuestSessionExpired({ ...session, operator: false }, Date.parse(joinResponse.expires_at) + 1)).toBe(true);
    persistRoomGuestSession(null);
  });

  it("uses canonical room metadata and does not hide pre-join history", () => {
    const session = roomGuestSessionFromJoinPayload("aaj1_test", {
      ...joinResponse,
    });

    const room = roomFromGuestSession(session);

    expect(room.label).toBe("Night Council");
    expect(room.topic).toBe("Old elevator");
    expect(room.createdAt).toBe("2026-07-10T00:00:00Z");
    expect(roomDockIdentity(room)).toBe(
      "11111111-1111-4111-8111-111111111111:room-1"
    );
  });

  it("does not restore a guest session after leave clears it", () => {
    const stored = new Map<string, string>();
    const originalStorage = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: {
        getItem: (key: string) => stored.get(key) ?? null,
        removeItem: (key: string) => stored.delete(key),
        setItem: (key: string, value: string) => stored.set(key, value),
      },
    });
    try {
      const session = roomGuestSessionFromJoinPayload("aaj1_test", {
        ...joinResponse,
      });

      persistRoomGuestSession(session);
      expect(loadRoomGuestSession()?.agentId).toBe("guest-1");

      persistRoomGuestSession(null);
      expect(loadRoomGuestSession()).toBeNull();
    } finally {
      if (originalStorage) {
        Object.defineProperty(window, "localStorage", originalStorage);
      }
    }
  });

  it("keeps a session active until its exact server expiry", () => {
    const expiresAt = Date.parse(joinResponse.expires_at);

    expect(
      roomGuestSessionExpired({ expiresAt: joinResponse.expires_at }, expiresAt - 1)
    ).toBe(false);
    expect(
      roomGuestSessionExpired({ expiresAt: joinResponse.expires_at }, expiresAt)
    ).toBe(true);
  });

  it("rejects a session write that durable storage cannot read back", () => {
    const originalStorage = Object.getOwnPropertyDescriptor(window, "localStorage");
    Object.defineProperty(window, "localStorage", {
      configurable: true,
      value: {
        getItem: () => null,
        removeItem: () => undefined,
        setItem: () => undefined,
      },
    });
    try {
      const session = roomGuestSessionFromJoinPayload("aaj1_test", {
        ...joinResponse,
      });

      expect(() => persistRoomGuestSession(session)).toThrow(/영구 저장/);
      expect(loadRoomGuestSession()).toBeNull();
    } finally {
      if (originalStorage) {
        Object.defineProperty(window, "localStorage", originalStorage);
      }
    }
  });
});

describe("guest room storage with several hosts live", () => {
  const HOST_X = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
  const HOST_Y = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
  const sessionForHostX = () =>
    roomGuestSessionFromJoinPayload("aaj1_host_x", {
      status: "admitted",
      request_id: "123e4567-e89b-42d3-a456-426614174000",
      session_token: "session-x",
      meeting_id: "room-x",
      agent_id: "guest-x",
      display_name: "Guest X",
      invite_scope: "room",
      participant_type: "human",
      client_type: "browser",
      provider_kind: "manual",
      owner_display_name: "",
      owner_id: "owner-x",
      stable_identity: false,
      operator: false,
      connection_kind: "native_remote_room_client",
      client_id: "client-x",
      expires_at: "2026-07-12T00:00:00Z",
      room_label: "Host X Room",
      room_topic: "",
      room_created_at: "2026-07-10T00:00:00Z",
      server_id: HOST_X,
      authority_lineage_id: "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
      server_product_surface: TEST_SERVER_PRODUCT_SURFACE,
      guide: {
        welcome: "Welcome",
        how_to: ["Read and send messages."],
        etiquette: [],
        session: { expires_in_seconds: 3600, rejoin: "Request another invite." },
      },
    });

  function installMemberHost(serverId: string, sessionToken: string) {
    const callbacks = new Set<() => void>();
    const transport = {
      hello: { origin: `https://${serverId}.test` },
      active: true,
      fetch: vi.fn(),
      onClose: (fn: () => void) => {
        callbacks.add(fn);
        return () => {
          callbacks.delete(fn);
        };
      },
      close: () => {
        transport.active = false;
        for (const fn of callbacks) fn();
      },
    };
    installRemoteWorkspace({
      transport: transport as unknown as RemoteTransport,
      owner: null,
      member: {
        sessionToken,
        serverSurface: { server_id: serverId },
      } as never,
      deviceToken: "device",
      clientId: "client",
    });
  }

  afterEach(() => {
    closeRemoteWorkspace();
    persistRoomGuestSession(null);
  });

  it("keeps a legacy guest room for host X visible while an unrelated host Y is live", () => {
    persistRoomGuestSession(sessionForHostX());
    installMemberHost(HOST_Y, "live-y-token");
    expect(loadRoomGuestSession()?.sessionToken).toBe("session-x");
    // Storing for X is not blocked by Y staying live either.
    persistRoomGuestSession(sessionForHostX());
    expect(loadRoomGuestSession()?.meetingId).toBe("room-x");
  });

  it("shadows the stored session only while its own host is live", () => {
    persistRoomGuestSession(sessionForHostX());
    installMemberHost(HOST_Y, "live-y-token");
    installMemberHost(HOST_X, "live-x-token");
    expect(loadRoomGuestSession()).toBeNull();
    closeRemoteWorkspace(HOST_X);
    expect(loadRoomGuestSession()?.sessionToken).toBe("session-x");
  });

  it("never blocks clearing stored state because another host stays live", () => {
    persistRoomGuestSession(sessionForHostX());
    installMemberHost(HOST_Y, "live-y-token");
    persistRoomGuestSession(null);
    expect(loadRoomGuestSession()).toBeNull();
  });
});
