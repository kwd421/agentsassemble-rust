import { useCallback, useRef } from "react";

import { createRoom } from "../api";
import { ApiError, userFacingError } from "../lib/apiErrors";
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
  create?: typeof createRoom;
  captureRoomDirectoryContinuity: () => RoomDirectoryContinuity;
  validateRoomDirectoryContinuity: (continuity: RoomDirectoryContinuity) => void;
  refreshRoomDirectory: (
    continuity: RoomDirectoryContinuity
  ) => Promise<RoomDirectoryRefreshResult>;
  verifyRoomDirectoryAuthority: (
    authority: RoomDirectoryAuthority,
    continuity: RoomDirectoryContinuity
  ) => Promise<RoomDirectoryVerificationResult>;
  onCreated: (room: RoomDockItem) => void | Promise<void>;
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
  return userFacingError(error, "방을 만들지 못했어요.");
}

export function useRoomCreation({
  guestLocked,
  create = createRoom,
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
      let lastError: unknown = new Error("방 생성 응답을 확인하지 못했어요.");
      for (let attempt = 0; attempt < 2; attempt += 1) {
        try {
          validateRoomDirectoryContinuity(continuity);
          const dispatchContinuity = continuity;
          const response = await create(
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
    [create, validateRoomDirectoryContinuity, verifyRoomDirectoryAuthority]
  );

  const addFreshRoom = useCallback(async (name = "새 방") => {
    if (guestLocked || inFlightRef.current) return false;
    inFlightRef.current = true;
    let continuity: RoomDirectoryContinuity | null = null;
    try {
      continuity = captureRoomDirectoryContinuity();
      if (pendingRef.current && pendingRef.current.room.label !== name.trim()) {
        throw new Error(`‘${pendingRef.current.room.label}’ 방 생성 결과를 먼저 확인해야 해요. 같은 이름으로 다시 시도해 주세요.`);
      }
      const intent = pendingRef.current || {
        requestId: createSecureRequestId(),
        room: { ...createFreshRoom(), label: name.trim() },
      };
      pendingRef.current = intent;

      const submitted = await submitExactIntent(intent, continuity);
      validateRoomDirectoryContinuity(submitted.continuity);
      continuity = submitted.continuity;
      if (!submitted.ok) {
        window.alert(errorMessage(submitted.error));
        return false;
      }

      const refreshed = await refreshRoomDirectory(continuity);
      validateRoomDirectoryContinuity(refreshed.continuity);
      continuity = refreshed.continuity;
      if (!refreshed.ok) {
        window.alert(errorMessage(refreshed.error));
        return false;
      }
      const canonicalRoom = refreshed.rooms.find(
        (room) => room.meetingId === intent.room.meetingId
      );
      if (!canonicalRoom) {
        validateRoomDirectoryContinuity(continuity);
        window.alert("만든 방을 목록에서 아직 확인하지 못했어요. 다시 시도해 주세요.");
        return false;
      }
      validateRoomDirectoryContinuity(continuity);
      pendingRef.current = null;
      await onCreated(canonicalRoom);
      return true;
    } catch (error) {
      if (error instanceof RoomDirectoryOperationSuperseded) return false;
      if (continuity) {
        try {
          validateRoomDirectoryContinuity(continuity);
        } catch (validationError) {
          if (validationError instanceof RoomDirectoryOperationSuperseded) return false;
          throw validationError;
        }
      }
      window.alert(errorMessage(error));
      return false;
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
