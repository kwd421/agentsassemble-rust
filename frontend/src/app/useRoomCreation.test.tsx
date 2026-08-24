import { act, renderHook } from "@testing-library/react";
import { Sparkles } from "lucide-react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { RoomDockItem } from "../lib/roomDockModel";
import { useRoomCreation } from "./useRoomCreation";

const apiMocks = vi.hoisted(() => ({ createRoom: vi.fn() }));

vi.mock("../api", async () => ({
  ...(await vi.importActual<typeof import("../api")>("../api")),
  createRoom: apiMocks.createRoom,
}));

function response(roomId: string) {
  return {
    status: "ready",
    server_id: "10000000-0000-4000-8000-000000000001",
    authority_lineage_id: "20000000-0000-4000-8000-000000000002",
    room: {
      room_id: roomId,
      room_uid: "30000000-0000-4000-8000-000000000003",
      label: "새 회의실",
    },
    deduplicated: true,
  };
}

function canonical(roomId: string): RoomDockItem {
  return {
    id: `server-${roomId}`,
    label: "새 회의실",
    meetingId: roomId,
    topic: "새 회의실",
    shortLabel: "새",
    icon: Sparkles,
    createdAt: "2026-08-25T00:00:00Z",
    tone: "resident",
  };
}

describe("useRoomCreation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(window, "alert").mockImplementation(() => undefined);
  });

  it("replays an ambiguous POST with the exact request before accepting the directory", async () => {
    apiMocks.createRoom
      .mockRejectedValueOnce(new TypeError("response lost"))
      .mockImplementationOnce(async (_requestId: string, roomId: string) => response(roomId));
    const refreshRoomDirectory = vi.fn(async () => [
      canonical(apiMocks.createRoom.mock.calls[0][1]),
    ]);
    const verifyRoomDirectoryAuthority = vi.fn(async () => undefined);
    const onCreated = vi.fn();
    const { result } = renderHook(() =>
      useRoomCreation({
        guestLocked: false,
        refreshRoomDirectory,
        verifyRoomDirectoryAuthority,
        onCreated,
      })
    );

    await act(result.current.addFreshRoom);

    expect(apiMocks.createRoom).toHaveBeenCalledTimes(2);
    expect(apiMocks.createRoom.mock.calls[1]).toEqual(apiMocks.createRoom.mock.calls[0]);
    expect(verifyRoomDirectoryAuthority).toHaveBeenCalledOnce();
    expect(refreshRoomDirectory).toHaveBeenCalledOnce();
    expect(onCreated).toHaveBeenCalledWith(
      expect.objectContaining({ meetingId: apiMocks.createRoom.mock.calls[0][1] })
    );
  });

  it("retains one pending intent across user retries after two ambiguous failures", async () => {
    apiMocks.createRoom
      .mockRejectedValueOnce(new TypeError("request lost"))
      .mockRejectedValueOnce(new TypeError("response lost"))
      .mockImplementationOnce(async (_requestId: string, roomId: string) => response(roomId));
    const refreshRoomDirectory = vi.fn(async () => [
      canonical(apiMocks.createRoom.mock.calls[0][1]),
    ]);
    const onCreated = vi.fn();
    const { result } = renderHook(() =>
      useRoomCreation({
        guestLocked: false,
        refreshRoomDirectory,
        verifyRoomDirectoryAuthority: vi.fn(async () => undefined),
        onCreated,
      })
    );

    await act(result.current.addFreshRoom);
    const firstIntent = apiMocks.createRoom.mock.calls[0];
    expect(window.alert).toHaveBeenCalledOnce();
    await act(result.current.addFreshRoom);

    expect(apiMocks.createRoom).toHaveBeenCalledTimes(3);
    expect(apiMocks.createRoom.mock.calls[2]).toEqual(firstIntent);
    expect(onCreated).toHaveBeenCalledOnce();
  });
});
