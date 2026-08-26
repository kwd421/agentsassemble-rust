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

  it("uses canonical room metadata and does not hide pre-join history", () => {
    const session = roomGuestSessionFromJoinPayload("aaj1_test", {
      ...serverSurface,
      session_token: "session-token",
      meeting_id: "room-1",
      agent_id: "guest-1",
      display_name: "Guest",
      invite_scope: "room",
      expires_at: "2026-07-12T00:00:00Z",
      room_label: "Night Council",
      room_topic: "Old elevator",
      room_created_at: "2026-07-10T00:00:00Z",
      room_uid: "room-uid-1",
      server_id: "11111111-1111-4111-8111-111111111111",
    });

    const room = roomFromGuestSession(session);

    expect(room.label).toBe("Night Council");
    expect(room.topic).toBe("Old elevator");
    expect(room.createdAt).toBe("2026-07-10T00:00:00Z");
    expect(roomDockIdentity(room)).toBe(
      "11111111-1111-4111-8111-111111111111:room-uid-1"
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
        ...serverSurface,
        session_token: "session-token",
        meeting_id: "room-1",
        agent_id: "guest-1",
        display_name: "Guest",
        invite_scope: "room",
        expires_at: "2026-07-12T00:00:00Z",
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
});
