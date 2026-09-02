import { useCallback, useRef } from "react";

import { createRoom } from "../api";
import { ApiError } from "../lib/apiErrors";
import { createFreshRoom, type RoomDockItem } from "../lib/roomDockModel";
import type { RoomDirectoryAuthority } from "../lib/roomDirectoryContract";
import { createSecureRequestId } from "../lib/secureRequestId";
import {
  RoomDirectoryOperationSuperseded,
  type RoomDirectoryContinuity,
  type RoomDirectoryRefreshResult,
  type RoomDirectoryVerificationResult,
} from "./useRoomDirectory";

type PendingRoomCreation = {
  requestId: string;
  room: RoomDockItem;
};

type UseRoomCreationOptions = {
  guestLocked: boolean;
  captureRoomDirectoryContinuity: () => RoomDirectoryContinuity;
  validateRoomDirectoryContinuity: (continuity: RoomDirectoryContinuity) => void;
  refreshRoomDirectory: (
    continuity: RoomDirectoryContinuity
  ) => Promise<RoomDirectoryRefreshResult>;
  verifyRoomDirectoryAuthority: (
    authority: RoomDirectoryAuthority,
    continuity: RoomDirectoryContinuity
  ) => Promise<RoomDirectoryVerificationResult>;
  onCreated: (room: RoomDockItem) => void;
};

type SubmitResult =
  | { ok: true; continuity: RoomDirectoryContinuity }
  | { ok: false; continuity: RoomDirectoryContinuity; error: unknown };

function ambiguousFailure(error: unknown): boolean {
  return !(
    error instanceof ApiError &&
    error.status !== 408 &&
    error.status !== 429 &&
    error.status < 500
  );
}

function errorMessage(error: unknown) {
  return error instanceof Error ? error.message : "방을 만들지 못했습니다.";
}

export function useRoomCreation({
  guestLocked,
  captureRoomDirectoryContinuity,
  validateRoomDirectoryContinuity,
  refreshRoomDirectory,
  verifyRoomDirectoryAuthority,
  onCreated,
}: UseRoomCreationOptions) {
  const pendingRef = useRef<PendingRoomCreation | null>(null);
  const inFlightRef = useRef(false);

  const submitExactIntent = useCallback(
    async (
      intent: PendingRoomCreation,
      initialContinuity: RoomDirectoryContinuity
    ): Promise<SubmitResult> => {
      let continuity = initialContinuity;
      let lastError: unknown = new Error("방 생성 응답을 확인하지 못했습니다.");
      for (let attempt = 0; attempt < 2; attempt += 1) {
        try {
          validateRoomDirectoryContinuity(continuity);
          const dispatchContinuity = continuity;
          const response = await createRoom(
            intent.requestId,
            intent.room.meetingId,
            intent.room.label,
            () => validateRoomDirectoryContinuity(dispatchContinuity)
          );
          validateRoomDirectoryContinuity(dispatchContinuity);
          const verification = await verifyRoomDirectoryAuthority(
            response,
            dispatchContinuity
          );
          validateRoomDirectoryContinuity(verification.continuity);
          continuity = verification.continuity;
          if (!verification.ok) throw verification.error;
          if (
            response.room.room_id !== intent.room.meetingId ||
            response.room.label !== intent.room.label
          ) {
            throw new Error("방 생성 결과가 보낸 요청과 일치하지 않습니다.");
          }
          return { ok: true, continuity };
        } catch (error) {
          if (error instanceof RoomDirectoryOperationSuperseded) throw error;
          validateRoomDirectoryContinuity(continuity);
          lastError = error;
          if (!ambiguousFailure(error)) {
            pendingRef.current = null;
            return { ok: false, continuity, error };
          }
        }
      }
      return { ok: false, continuity, error: lastError };
    },
    [validateRoomDirectoryContinuity, verifyRoomDirectoryAuthority]
  );

  const addFreshRoom = useCallback(async () => {
    if (guestLocked || inFlightRef.current) return;
    inFlightRef.current = true;
    let continuity: RoomDirectoryContinuity | null = null;
    try {
      continuity = captureRoomDirectoryContinuity();
      const intent = pendingRef.current || {
        requestId: createSecureRequestId(),
        room: createFreshRoom(),
      };
      pendingRef.current = intent;

      const submitted = await submitExactIntent(intent, continuity);
      validateRoomDirectoryContinuity(submitted.continuity);
      continuity = submitted.continuity;
      if (!submitted.ok) {
        window.alert(errorMessage(submitted.error));
        return;
      }

      const refreshed = await refreshRoomDirectory(continuity);
      validateRoomDirectoryContinuity(refreshed.continuity);
      continuity = refreshed.continuity;
      if (!refreshed.ok) {
        window.alert(errorMessage(refreshed.error));
        return;
      }
      const canonicalRoom = refreshed.rooms.find(
        (room) => room.meetingId === intent.room.meetingId
      );
      if (!canonicalRoom) {
        validateRoomDirectoryContinuity(continuity);
        window.alert("생성된 방이 권위 있는 방 목록에 없습니다.");
        return;
      }
      validateRoomDirectoryContinuity(continuity);
      pendingRef.current = null;
      onCreated(canonicalRoom);
    } catch (error) {
      if (error instanceof RoomDirectoryOperationSuperseded) return;
      if (continuity) {
        try {
          validateRoomDirectoryContinuity(continuity);
        } catch (validationError) {
          if (validationError instanceof RoomDirectoryOperationSuperseded) return;
          throw validationError;
        }
      }
      window.alert(errorMessage(error));
    } finally {
      inFlightRef.current = false;
    }
  }, [
    captureRoomDirectoryContinuity,
    guestLocked,
    onCreated,
    refreshRoomDirectory,
    submitExactIntent,
    validateRoomDirectoryContinuity,
  ]);

  return { addFreshRoom };
}
