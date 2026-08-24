import { useCallback, useRef } from "react";

import { createRoom } from "../api";
import { ApiError } from "../lib/apiErrors";
import {
  createFreshRoom,
  type RoomDockItem,
} from "../lib/roomDockModel";
import type { RoomDirectoryAuthority } from "../lib/roomDirectoryContract";

type PendingRoomCreation = {
  requestId: string;
  room: RoomDockItem;
};

type UseRoomCreationOptions = {
  guestLocked: boolean;
  refreshRoomDirectory: () => Promise<RoomDockItem[]>;
  verifyRoomDirectoryAuthority: (authority: RoomDirectoryAuthority) => Promise<void>;
  onCreated: (room: RoomDockItem) => void;
};

function ambiguousFailure(error: unknown): boolean {
  return !(
    error instanceof ApiError &&
    error.status !== 408 &&
    error.status !== 429 &&
    error.status < 500
  );
}

export function useRoomCreation({
  guestLocked,
  refreshRoomDirectory,
  verifyRoomDirectoryAuthority,
  onCreated,
}: UseRoomCreationOptions) {
  const pendingRef = useRef<PendingRoomCreation | null>(null);
  const inFlightRef = useRef(false);

  const submitExactIntent = useCallback(
    async (intent: PendingRoomCreation) => {
      let lastError: unknown = new Error("방 생성 응답을 확인하지 못했습니다.");
      for (let attempt = 0; attempt < 2; attempt += 1) {
        try {
          const response = await createRoom(
            intent.requestId,
            intent.room.meetingId,
            intent.room.label
          );
          await verifyRoomDirectoryAuthority(response);
          if (
            response.room.room_id !== intent.room.meetingId ||
            response.room.label !== intent.room.label
          ) {
            throw new Error("방 생성 결과가 보낸 요청과 일치하지 않습니다.");
          }
          return;
        } catch (error) {
          lastError = error;
          if (!ambiguousFailure(error)) {
            pendingRef.current = null;
            throw error;
          }
        }
      }
      throw lastError;
    },
    [verifyRoomDirectoryAuthority]
  );

  const addFreshRoom = useCallback(async () => {
    if (guestLocked || inFlightRef.current) return;
    const intent = pendingRef.current || {
      requestId: globalThis.crypto.randomUUID(),
      room: createFreshRoom(),
    };
    pendingRef.current = intent;
    inFlightRef.current = true;
    try {
      await submitExactIntent(intent);
      const synchronized = await refreshRoomDirectory();
      const canonicalRoom = synchronized.find(
        (room) => room.meetingId === intent.room.meetingId
      );
      if (!canonicalRoom) {
        throw new Error("생성된 방이 권위 있는 방 목록에 없습니다.");
      }
      pendingRef.current = null;
      onCreated(canonicalRoom);
    } catch (error) {
      window.alert(error instanceof Error ? error.message : "방을 만들지 못했습니다.");
    } finally {
      inFlightRef.current = false;
    }
  }, [guestLocked, onCreated, refreshRoomDirectory, submitExactIntent]);

  return { addFreshRoom };
}
