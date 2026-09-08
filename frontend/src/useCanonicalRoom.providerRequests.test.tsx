import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { RoomEvent } from "./api";
import type { RoomSocketHandlers, RoomSocketHandle, RoomSocketSnapshot } from "./roomSocketClient";
import { pendingRequest } from "./test/providerRequest";
import { useCanonicalRoom } from "./useCanonicalRoom";
import { handshakeFrames } from "./test/roomSocketHarness";
import { TEST_SERVER_PRODUCT_SURFACE } from "./test/serverProductSurface";
import { publicRoomEventIsValid } from "./lib/roomSocketValidation";


const baseEvent = { v: 1, id: "evt", seq: 1, room_id: "general", created_at: "2026-09-08T22:00:00Z", actor: { participant_id: "agent", participant_type: "agent" }, visibility: "owner", owner_id: "operator-local" };

describe("owned provider request projection", () => {
  it("replaces from every accepted snapshot and follows only current-owner live transitions", async () => {
    let handlers!: RoomSocketHandlers;
    const openSocket = vi.fn((_auth, _streams, next: RoomSocketHandlers) => {
      handlers = next;
      return { close: vi.fn(), resync: vi.fn(), ready: () => true, command: vi.fn(), say: vi.fn(), resolveProviderRequest: vi.fn(), historyBefore: vi.fn() } satisfies RoomSocketHandle;
    });
    const { result, rerender } = renderHook(({ roomId }) => useCanonicalRoom({ roomId, viewerParticipantId: "operator-local", auth: { kind: "host", meetingId: roomId }, serverSurface: TEST_SERVER_PRODUCT_SURFACE, openSocket }), { initialProps: { roomId: "general" } });
    await waitFor(() => expect(openSocket).toHaveBeenCalledOnce());
    const snapshot = { ...handshakeFrames(0, 0).snap, provider_requests: [pendingRequest] } as RoomSocketSnapshot;
    act(() => { handlers.onRoomSnapshot?.(snapshot, "http://127.0.0.1:43123"); });
    expect(result.current.providerRequests).toEqual([pendingRequest]);
    const resolving = { ...baseEvent, type: "provider_request_resolving", provider_request_id: pendingRequest.request.provider_request_id } as RoomEvent;
    act(() => { handlers.onRoomEvents?.([resolving]); });
    expect(result.current.providerRequests[0].state).toBe("resolving");
    const closed = { ...baseEvent, seq: 2, id: "closed", type: "provider_request_closed", provider_request_id: pendingRequest.request.provider_request_id, state: "failed" } as RoomEvent;
    act(() => { handlers.onRoomEvents?.([{ ...closed, owner_id: "other" } as RoomEvent]); });
    expect(result.current.providerRequests).toHaveLength(1);
    act(() => { handlers.onRoomSnapshot?.({ ...snapshot, snapshot_mode: "resume", provider_requests: [] }, "http://127.0.0.1:43123"); });
    expect(result.current.providerRequests).toEqual([]);
    const opened = { ...baseEvent, seq: 3, id: "open", type: "provider_request_opened", session_id: "session", provider_request: pendingRequest.request, expires_at: pendingRequest.expires_at } as unknown as RoomEvent;
    act(() => { handlers.onRoomEvents?.([opened]); });
    expect(result.current.providerRequests).toEqual([pendingRequest]);
    act(() => { handlers.onRoomEvents?.([{ ...closed, seq: 4 } as RoomEvent]); });
    expect(result.current.providerRequests).toEqual([]);
    act(() => { handlers.onRoomSnapshot?.(snapshot, "http://127.0.0.1:43123"); });
    rerender({ roomId: "other" });
    expect(result.current.providerRequests).toEqual([]);
  });

  it("rejects malformed native-request events at the socket boundary", () => {
    const event = { ...baseEvent, type: "provider_request_opened", session_id: "session", provider_request: pendingRequest.request, expires_at: pendingRequest.expires_at };
    expect(publicRoomEventIsValid(event, "general")).toBe(true);
    expect(publicRoomEventIsValid({ ...event, session_id: null }, "general")).toBe(false);
    expect(publicRoomEventIsValid({ ...event, provider_request: {} }, "general")).toBe(false);
    expect(publicRoomEventIsValid({ ...event, expires_at: "invalid" }, "general")).toBe(false);
    expect(publicRoomEventIsValid({ ...baseEvent, type: "provider_request_closed", provider_request_id: "id", state: "unknown" }, "general")).toBe(false);
  });
});
