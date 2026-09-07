import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { changeRoomLifecycle } from "../api/roomLifecycle";
import { ApiError } from "../lib/apiErrors";
import { useRoomLifecycle } from "./useRoomLifecycle";
import type { RoomDirectoryContinuity } from "./useRoomDirectory";

vi.mock("../api/roomLifecycle", () => ({ changeRoomLifecycle: vi.fn() }));
vi.mock("../lib/roomDirectoryContract", async (load) => ({
  ...await load<object>(),
  currentRoomDirectoryAuthority: () => ({ server_id: "server", authority_lineage_id: "lineage" }),
}));

describe("room lifecycle request ownership", () => {
  it("retains exact uncertain intent, suppresses concurrent refresh and retries without retargeting", async () => {
    const continuity = {} as RoomDirectoryContinuity;
    const refreshRoomDirectory = vi.fn(async () => ({ ok: true as const, continuity, rooms: [] }));
    const room = { room_id: "general", room_uid: "exact-room", label: "General", status: "active" };
    let reject!: (error: Error) => void;
    vi.mocked(changeRoomLifecycle).mockImplementationOnce((_intent, beforeDispatch) => {
      beforeDispatch();
      return new Promise((_resolve, fail) => { reject = fail; });
    });
    const { result } = renderHook(() => useRoomLifecycle({ enabled: true, authorityReady: true, managementRooms: [room],
      captureRoomDirectoryContinuity: () => continuity, validateRoomDirectoryContinuity: vi.fn(), refreshRoomDirectory }));
    act(() => { result.current.change(room, "archive"); result.current.onRoomLifecycle(); });
    expect(refreshRoomDirectory).not.toHaveBeenCalled();
    act(() => reject(new Error("Response was lost")));
    await waitFor(() => expect(result.current.busy).toBe(false));
    const intent = vi.mocked(changeRoomLifecycle).mock.calls[0][0];
    expect(result.current.pending).toEqual(intent);
    act(() => result.current.change({ ...room, room_uid: "replacement" }, "close"));
    expect(changeRoomLifecycle).toHaveBeenCalledTimes(1);
    vi.mocked(changeRoomLifecycle).mockResolvedValueOnce({ room: { room_id: "general", room_uid: "exact-room", label: "General", status: "archived", created_at: "", updated_at: "" }, cleanupPending: true });
    act(() => result.current.retry());
    await waitFor(() => expect(result.current.pending).toBeNull());
    expect(vi.mocked(changeRoomLifecycle).mock.calls[1][0]).toEqual(intent);
    expect(refreshRoomDirectory).toHaveBeenCalledTimes(2);
    expect(result.current.notice).toContain("정리");
    vi.mocked(changeRoomLifecycle).mockRejectedValueOnce(new ApiError(409, "stale room", "room_incarnation_changed", "rejected"));
    act(() => result.current.change(room, "close"));
    await waitFor(() => expect(result.current.error).toBe("stale room"));
    expect(result.current.pending).toBeNull();
  });
});
