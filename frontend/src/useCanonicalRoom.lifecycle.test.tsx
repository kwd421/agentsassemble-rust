import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RoomEvent } from "./api";
import type { RoomSocketHandlers, RoomSocketHandle, RoomSocketSnapshot } from "./roomSocketClient";
import { useCanonicalRoom } from "./useCanonicalRoom";
import { handshakeFrames } from "./test/roomSocketHarness";
import { TEST_SERVER_PRODUCT_SURFACE } from "./test/serverProductSurface";

describe("room lifecycle notification", () => {
  it("checks the subscribed incarnation before invalidating the directory and closing", async () => {
    let handlers!: RoomSocketHandlers;
    const onRoomLifecycle = vi.fn();
    const close = vi.fn();
    const resync = vi.fn();
    const openSocket = vi.fn((_auth, _streams, next: RoomSocketHandlers) => {
      handlers = next;
      return { close, resync, ready: () => true, command: vi.fn(), say: vi.fn(), historyBefore: vi.fn() } satisfies RoomSocketHandle;
    });
    renderHook(() => useCanonicalRoom({ roomId: "general", auth: { kind: "host", meetingId: "general" }, serverSurface: TEST_SERVER_PRODUCT_SURFACE, openSocket, onRoomLifecycle }));
    await waitFor(() => expect(openSocket).toHaveBeenCalledOnce());
    const snapshot = { ...handshakeFrames(0, 0).snap, room: { ...handshakeFrames(0, 0).snap.room, room_uid: "exact-room" } } as RoomSocketSnapshot;
    act(() => { handlers.onRoomSnapshot?.(snapshot, "http://127.0.0.1:43123"); });
    const room = { room_id: "general", room_uid: "wrong-room", label: "General", status: "archived", created_at: "2026-09-07T00:00:00Z", updated_at: "2026-09-07T00:00:01Z" };
    const event = { id: "archived", v: 1, room_id: "general", seq: 1, created_at: room.updated_at, type: "room_archived", actor: { participant_id: "operator-local", participant_type: "human" }, room } as RoomEvent;
    act(() => { handlers.onRoomEvents?.([event]); });
    expect(onRoomLifecycle).not.toHaveBeenCalled();
    expect(resync).toHaveBeenCalledOnce();
    act(() => { handlers.onRoomEvents?.([{ ...event, room: { ...room, room_uid: "exact-room" } } as RoomEvent]); });
    expect(onRoomLifecycle).toHaveBeenCalledWith({ ...room, room_uid: "exact-room" });
    expect(close).toHaveBeenCalledOnce();
  });
});
