import { describe, expect, it } from "vitest";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";
import { roomDockIdentity, roomFromGuestSession } from "./roomDockModel";
import {
  loadRoomGuestSession,
  persistRoomGuestSession,
  roomGuestSessionFromJoinPayload,
} from "./roomGuestSession";

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

  it("uses canonical room metadata and does not hide pre-join history", () => {
    const session = roomGuestSessionFromJoinPayload("aaj1_test", {
      ...joinResponse,
    });

    const room = roomFromGuestSession(session);

    expect(room.label).toBe("Night Council");
    expect(room.topic).toBe("Old elevator");
    expect(room.createdAt).toBe("2026-07-10T00:00:00Z");
    expect(roomDockIdentity(room)).toBe(
      "remote:http://localhost:3000:room-1"
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
