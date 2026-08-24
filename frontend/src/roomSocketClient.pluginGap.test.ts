import { describe, expect, it, vi } from "vitest";
import { openRoomSocket } from "./roomSocketClient";
import { TEST_SERVER_PRODUCT_SURFACE } from "./test/serverProductSurface";

describe("closed WebSocket product surface", () => {
  it("does not open a socket for the unimplemented plugin stream", async () => {
    const createSocket = vi.fn();
    const onError = vi.fn();
    const handle = openRoomSocket(
      { kind: "host", meetingId: "general" },
      ["room_events", "plugin"],
      { onError },
      {
        getTicket: async () => ({
          ticket: "a".repeat(64),
          websocket_base_url: "ws://127.0.0.1:43123",
          server_proof_key: "b".repeat(64),
        }),
        createSocket,
        serverSurface: TEST_SERVER_PRODUCT_SURFACE,
        expectedRoomId: "general",
        expectedParticipantId: "operator-local",
      }
    );
    await Promise.resolve();
    await Promise.resolve();
    expect(createSocket).not.toHaveBeenCalled();
    expect(onError).toHaveBeenCalledWith(
      expect.objectContaining({ category: "surface_stream_unavailable" })
    );
    handle.close();
  });
});
