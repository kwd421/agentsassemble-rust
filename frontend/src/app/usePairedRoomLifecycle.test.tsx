import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { changeRoomLifecycle } from "../api/roomLifecycle";
import { ApiError } from "../lib/apiErrors";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import { roomFixture } from "../test/room";
import { TEST_SERVER_PRODUCT_SURFACE } from "../test/serverProductSurface";
import { usePairedRoomLifecycle } from "./usePairedRoomLifecycle";

vi.mock("../api/roomLifecycle", () => ({ changeRoomLifecycle: vi.fn() }));
beforeEach(() => vi.resetAllMocks());
afterEach(cleanup);
const room = roomFixture();
const session: RoomGuestSession = {
  sessionToken: "aops1.paired", inviteToken: "", meetingId: room.room_id,
  roomUid: room.room_uid, agentId: "operator-local", displayName: "Host",
  inviteScope: "room", expiresAt: "2099-01-01T00:00:00Z", joinedAt: "2026-09-08T00:00:00Z",
  serverSurface: { server_id: "server", authority_lineage_id: "lineage", server_product_surface: TEST_SERVER_PRODUCT_SURFACE },
};
const initial: Parameters<typeof usePairedRoomLifecycle>[0] = {
  enabled: true, session, deviceToken: "first-device", expired: false, room, refreshProjection: vi.fn(),
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
  return { promise, resolve, reject };
}

it.each([false, true])("retains the committed response across its own session expiry: %s", async (expiresFirst) => {
  const response = deferred<Awaited<ReturnType<typeof changeRoomLifecycle>>>();
  vi.mocked(changeRoomLifecycle).mockImplementationOnce((_intent, before) => { before(); return response.promise; });
  const hook = renderHook((options) => usePairedRoomLifecycle(options), { initialProps: initial });
  act(() => hook.result.current.show());
  act(() => { hook.result.current.change(room, "close"); hook.result.current.change(room, "archive"); });
  expect(changeRoomLifecycle).toHaveBeenCalledOnce();
  expect(vi.mocked(changeRoomLifecycle).mock.calls[0][2]).toEqual({ kind: "remote", sessionToken: session.sessionToken, deviceToken: "first-device" });
  const closed = roomFixture({ status: "closed" });
  if (expiresFirst) {
    act(() => hook.result.current.onRoomLifecycle(closed));
    hook.rerender({ ...initial, enabled: false, session: null, expired: true, room: null });
  }
  expect(hook.result.current.open).toBe(true);
  await act(async () => response.resolve({ room: closed, cleanupPending: false }));
  expect(hook.result.current.busy).toBe(false);
  expect(hook.result.current.notice).toContain("방 상태가 변경");
  expect(hook.result.current.rooms[0].status).toBe("closed");
  expect(hook.result.current.canChange).toBe(false);
});

it("retires an old device response before another viewer opens management", async () => {
  const response = deferred<Awaited<ReturnType<typeof changeRoomLifecycle>>>();
  vi.mocked(changeRoomLifecycle).mockReturnValueOnce(response.promise);
  const hook = renderHook((options) => usePairedRoomLifecycle(options), { initialProps: initial });
  act(() => hook.result.current.show());
  act(() => hook.result.current.change(room, "close"));
  hook.rerender({ ...initial, deviceToken: "second-device", session: { ...session, sessionToken: "aops1.other" } });
  expect(hook.result.current.open).toBe(false);
  expect(hook.result.current.pending).toBeNull();
  act(() => hook.result.current.show());
  await act(async () => response.resolve({ room: roomFixture({ status: "closed" }), cleanupPending: false }));
  expect(hook.result.current.notice).toBe("");
  expect(hook.result.current.rooms[0].status).toBe("active");
});

it("retries only the exact uncertain intent and distinguishes accepted deletion from completion", async () => {
  vi.mocked(changeRoomLifecycle).mockRejectedValueOnce(new Error("Response lost"));
  const hook = renderHook((options) => usePairedRoomLifecycle(options), { initialProps: initial });
  act(() => hook.result.current.show());
  act(() => hook.result.current.change(room, "delete", "General"));
  await waitFor(() => expect(hook.result.current.busy).toBe(false));
  const intent = hook.result.current.pending;
  expect(intent?.action).toBe("room.delete");
  act(() => hook.result.current.change(room, "archive"));
  expect(changeRoomLifecycle).toHaveBeenCalledOnce();
  const response = deferred<Awaited<ReturnType<typeof changeRoomLifecycle>>>();
  vi.mocked(changeRoomLifecycle).mockReturnValueOnce(response.promise);
  act(() => hook.result.current.retry());
  expect(vi.mocked(changeRoomLifecycle).mock.calls[1][0]).toEqual(intent);
  hook.rerender({ ...initial, enabled: false, session: null, expired: true, room: null });
  await act(async () => response.reject(new ApiError(503, "accepted", "room_deletion_pending", "unresolved")));
  expect(hook.result.current.notice).toContain("삭제 요청이 접수");
  expect(hook.result.current.notice).toContain("원래 앱");
  expect(hook.result.current.notice).not.toBe("방이 삭제됐습니다.");
  expect(hook.result.current.pending).toBeNull();
  expect(hook.result.current.busy).toBe(false);
  act(() => hook.result.current.retry());
  expect(changeRoomLifecycle).toHaveBeenCalledTimes(2);
});
