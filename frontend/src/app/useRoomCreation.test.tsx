import { act, renderHook } from "@testing-library/react";
import { Sparkles } from "lucide-react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../lib/apiErrors";
import type { RoomDockItem } from "../lib/roomDockModel";
import { useRoomCreation } from "./useRoomCreation";
import {
  RoomDirectoryOperationSuperseded,
  type RoomDirectoryContinuity,
} from "./useRoomDirectory";

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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

function continuity(): RoomDirectoryContinuity {
  return {} as RoomDirectoryContinuity;
}

describe("useRoomCreation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.spyOn(window, "alert").mockImplementation(() => undefined);
  });

  it("replays an ambiguous POST with the exact request before accepting the directory", async () => {
    const continuity = {} as RoomDirectoryContinuity;
    apiMocks.createRoom
      .mockRejectedValueOnce(new TypeError("response lost"))
      .mockImplementationOnce(async (_requestId: string, roomId: string) => response(roomId));
    const refreshRoomDirectory = vi.fn(async () => ({
      ok: true as const,
      continuity,
      rooms: [canonical(apiMocks.createRoom.mock.calls[0][1])],
    }));
    const verifyRoomDirectoryAuthority = vi.fn(async () => ({
      ok: true as const,
      continuity,
    }));
    const onCreated = vi.fn();
    const { result } = renderHook(() =>
      useRoomCreation({
        guestLocked: false,
        captureRoomDirectoryContinuity: () => continuity,
        validateRoomDirectoryContinuity: vi.fn(),
        refreshRoomDirectory,
        verifyRoomDirectoryAuthority,
        onCreated,
      })
    );

    await act(result.current.addFreshRoom);

    expect(apiMocks.createRoom).toHaveBeenCalledTimes(2);
    expect(apiMocks.createRoom.mock.calls[1].slice(0, 3)).toEqual(
      apiMocks.createRoom.mock.calls[0].slice(0, 3)
    );
    expect(verifyRoomDirectoryAuthority).toHaveBeenCalledOnce();
    expect(refreshRoomDirectory).toHaveBeenCalledOnce();
    expect(onCreated).toHaveBeenCalledWith(
      expect.objectContaining({ meetingId: apiMocks.createRoom.mock.calls[0][1] })
    );
  });

  it("retains one pending intent across user retries after two ambiguous failures", async () => {
    const continuity = {} as RoomDirectoryContinuity;
    apiMocks.createRoom
      .mockRejectedValueOnce(new TypeError("request lost"))
      .mockRejectedValueOnce(new TypeError("response lost"))
      .mockImplementationOnce(async (_requestId: string, roomId: string) => response(roomId));
    const refreshRoomDirectory = vi.fn(async () => ({
      ok: true as const,
      continuity,
      rooms: [canonical(apiMocks.createRoom.mock.calls[0][1])],
    }));
    const onCreated = vi.fn();
    const { result } = renderHook(() =>
      useRoomCreation({
        guestLocked: false,
        captureRoomDirectoryContinuity: () => continuity,
        validateRoomDirectoryContinuity: vi.fn(),
        refreshRoomDirectory,
        verifyRoomDirectoryAuthority: vi.fn(async () => ({
          ok: true as const,
          continuity,
        })),
        onCreated,
      })
    );

    await act(result.current.addFreshRoom);
    const firstIntent = apiMocks.createRoom.mock.calls[0];
    expect(window.alert).toHaveBeenCalledOnce();
    await act(result.current.addFreshRoom);

    expect(apiMocks.createRoom).toHaveBeenCalledTimes(3);
    expect(apiMocks.createRoom.mock.calls[2].slice(0, 3)).toEqual(
      firstIntent.slice(0, 3)
    );
    expect(onCreated).toHaveBeenCalledOnce();
  });

  it("advances continuity and replays the exact POST after a current verification failure", async () => {
    const captured = continuity();
    const failedVerification = continuity();
    const verified = continuity();
    const published = continuity();
    apiMocks.createRoom.mockImplementation(
      async (_requestId: string, roomId: string) => response(roomId)
    );
    const verifyRoomDirectoryAuthority = vi
      .fn()
      .mockResolvedValueOnce({
        ok: false as const,
        continuity: failedVerification,
        error: new Error("bootstrap response lost"),
      })
      .mockResolvedValueOnce({ ok: true as const, continuity: verified });
    const refreshRoomDirectory = vi.fn(async () => ({
      ok: true as const,
      continuity: published,
      rooms: [canonical(apiMocks.createRoom.mock.calls[0][1])],
    }));
    const onCreated = vi.fn();
    const { result } = renderHook(() =>
      useRoomCreation({
        guestLocked: false,
        captureRoomDirectoryContinuity: () => captured,
        validateRoomDirectoryContinuity: vi.fn(),
        refreshRoomDirectory,
        verifyRoomDirectoryAuthority,
        onCreated,
      })
    );

    await act(result.current.addFreshRoom);

    expect(apiMocks.createRoom).toHaveBeenCalledTimes(2);
    expect(apiMocks.createRoom.mock.calls[1].slice(0, 3)).toEqual(
      apiMocks.createRoom.mock.calls[0].slice(0, 3)
    );
    expect(window.alert).not.toHaveBeenCalled();
    expect(onCreated).toHaveBeenCalledOnce();
  });

  it("silently preserves the exact pending intent when a rejected POST is superseded", async () => {
    const first = deferred<ReturnType<typeof response>>();
    const firstContinuity = continuity();
    const retryContinuity = continuity();
    const verified = continuity();
    const published = continuity();
    let current: RoomDirectoryContinuity | null = firstContinuity;
    apiMocks.createRoom
      .mockReturnValueOnce(first.promise)
      .mockImplementationOnce(async (_requestId: string, roomId: string) => response(roomId));
    const validateRoomDirectoryContinuity = vi.fn(
      (candidate: RoomDirectoryContinuity) => {
        if (candidate !== current) throw new RoomDirectoryOperationSuperseded();
      }
    );
    const verifyRoomDirectoryAuthority = vi.fn(async () => {
      current = verified;
      return { ok: true as const, continuity: verified };
    });
    const refreshRoomDirectory = vi.fn(async () => {
      current = published;
      return {
        ok: true as const,
        continuity: published,
        rooms: [canonical(apiMocks.createRoom.mock.calls[0][1])],
      };
    });
    const onCreated = vi.fn();
    const { result } = renderHook(() =>
      useRoomCreation({
        guestLocked: false,
        captureRoomDirectoryContinuity: () => {
          if (!current) throw new RoomDirectoryOperationSuperseded();
          return current;
        },
        validateRoomDirectoryContinuity,
        refreshRoomDirectory,
        verifyRoomDirectoryAuthority,
        onCreated,
      })
    );

    let staleAttempt!: Promise<void>;
    await act(async () => {
      staleAttempt = result.current.addFreshRoom();
      await Promise.resolve();
    });
    const firstIntent = apiMocks.createRoom.mock.calls[0];
    current = null;
    first.reject(new TypeError("late transport rejection"));
    await act(async () => staleAttempt);

    expect(apiMocks.createRoom).toHaveBeenCalledOnce();
    expect(window.alert).not.toHaveBeenCalled();
    expect(onCreated).not.toHaveBeenCalled();

    current = retryContinuity;
    await act(result.current.addFreshRoom);
    expect(apiMocks.createRoom).toHaveBeenCalledTimes(2);
    expect(apiMocks.createRoom.mock.calls[1].slice(0, 3)).toEqual(
      firstIntent.slice(0, 3)
    );
    expect(onCreated).toHaveBeenCalledOnce();
  });

  it("clears a current terminal failure but preserves one superseded before classification", async () => {
    const late = deferred<ReturnType<typeof response>>();
    const firstContinuity = continuity();
    const retryContinuity = continuity();
    let current: RoomDirectoryContinuity | null = firstContinuity;
    apiMocks.createRoom
      .mockRejectedValueOnce(new ApiError(400, "invalid room"))
      .mockReturnValueOnce(late.promise)
      .mockImplementationOnce(async (_requestId: string, roomId: string) => response(roomId));
    const validateRoomDirectoryContinuity = vi.fn(
      (candidate: RoomDirectoryContinuity) => {
        if (candidate !== current) throw new RoomDirectoryOperationSuperseded();
      }
    );
    const verifyRoomDirectoryAuthority = vi.fn(async () => ({
      ok: true as const,
      continuity: current as RoomDirectoryContinuity,
    }));
    const refreshRoomDirectory = vi.fn(async () => ({
      ok: true as const,
      continuity: current as RoomDirectoryContinuity,
      rooms: [canonical(apiMocks.createRoom.mock.calls.at(-1)?.[1])],
    }));
    const onCreated = vi.fn();
    const { result } = renderHook(() => useRoomCreation({
      guestLocked: false,
      captureRoomDirectoryContinuity: () => current as RoomDirectoryContinuity,
      validateRoomDirectoryContinuity,
      refreshRoomDirectory,
      verifyRoomDirectoryAuthority,
      onCreated,
    }));

    await act(result.current.addFreshRoom);
    const terminalIntent = apiMocks.createRoom.mock.calls[0];
    expect(window.alert).toHaveBeenCalledWith("invalid room");

    let staleAttempt!: Promise<void>;
    await act(async () => {
      staleAttempt = result.current.addFreshRoom();
      await Promise.resolve();
    });
    const retainedIntent = apiMocks.createRoom.mock.calls[1];
    expect(retainedIntent[0]).not.toBe(terminalIntent[0]);
    current = null;
    late.reject(new ApiError(400, "late invalid room"));
    await act(async () => staleAttempt);
    expect(window.alert).toHaveBeenCalledTimes(1);

    current = retryContinuity;
    await act(result.current.addFreshRoom);
    expect(apiMocks.createRoom.mock.calls[2].slice(0, 3)).toEqual(
      retainedIntent.slice(0, 3)
    );
    expect(onCreated).toHaveBeenCalledOnce();
  });
});
