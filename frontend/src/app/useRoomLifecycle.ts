import { currentRoomDirectoryAuthority } from "../lib/roomDirectoryContract";
import { ApiError } from "../lib/apiErrors";
import { useCallback, useRef, useState } from "react";
import { changeRoomLifecycle, type RoomLifecycleIntent } from "../api/roomLifecycle";
import { createSecureRequestId } from "../lib/secureRequestId";
import type { ServerRoomDockSource } from "../lib/roomDockModel";
import type { useRoomDirectory } from "./useRoomDirectory";
import { RoomDirectoryOperationSuperseded } from "./useRoomDirectory";

type Directory = ReturnType<typeof useRoomDirectory>;
type Options = Pick<Directory, "managementRooms" | "captureRoomDirectoryContinuity" | "validateRoomDirectoryContinuity" | "refreshRoomDirectory"> & { enabled: boolean; authorityReady: boolean };

export function useRoomLifecycle({ enabled, authorityReady, managementRooms, captureRoomDirectoryContinuity, validateRoomDirectoryContinuity, refreshRoomDirectory }: Options) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const busyRef = useRef(false);
  const directoryInvalidatedRef = useRef(false);
  const pendingRef = useRef<RoomLifecycleIntent | null>(null);
  const [pending, setPending] = useState<RoomLifecycleIntent | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");

  const refresh = useCallback(async () => {
    if (!enabled || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    if (!pendingRef.current) setError("");
    try {
      do {
        directoryInvalidatedRef.current = false;
        const result = await refreshRoomDirectory(captureRoomDirectoryContinuity());
        if (!result.ok) throw result.error;
      } while (directoryInvalidatedRef.current);
    } catch (failure) {
      if (!(failure instanceof RoomDirectoryOperationSuperseded)) {
        setError(failure instanceof Error ? failure.message : "방 목록을 확인하지 못했습니다.");
      }
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }, [enabled, captureRoomDirectoryContinuity, refreshRoomDirectory]);

  const submit = useCallback(async (intent: RoomLifecycleIntent) => {
    if (!enabled || busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    setNotice("");
    pendingRef.current = intent;
    setPending(intent);
    try {
      const continuity = captureRoomDirectoryContinuity();
      const response = await changeRoomLifecycle(intent, () => validateRoomDirectoryContinuity(continuity));
      validateRoomDirectoryContinuity(continuity);
      pendingRef.current = null;
      setPending(null);
      setNotice(response.cleanupPending ? "방 상태가 변경됐습니다. 실행 중이던 에이전트 정리를 기다리고 있습니다." : "방 상태가 변경됐습니다.");
      directoryInvalidatedRef.current = false;
      const refreshed = await refreshRoomDirectory(continuity);
      if (!refreshed.ok) throw refreshed.error;
    } catch (failure) {
      if (failure instanceof ApiError && failure.resolution === "rejected") {
        pendingRef.current = null;
        setPending(null);
      }
      setError(failure instanceof Error ? failure.message : "방 관리 결과를 확인하지 못했습니다.");
    } finally {
      busyRef.current = false;
      setBusy(false);
      if (directoryInvalidatedRef.current) {
        directoryInvalidatedRef.current = false;
        void refresh();
      }
    }
  }, [enabled, captureRoomDirectoryContinuity, validateRoomDirectoryContinuity, refreshRoomDirectory, refresh]);

  function change(room: ServerRoomDockSource, action: "close" | "archive" | "restore") {
    if (!authorityReady || !room.room_uid || pendingRef.current || !managementRooms.some((candidate) => candidate.room_id === room.room_id && candidate.room_uid === room.room_uid)) return;
    const authority = currentRoomDirectoryAuthority();
    if (!authority) return;
    void submit({ serverId: authority.server_id, authorityLineageId: authority.authority_lineage_id, requestId: createSecureRequestId(), roomId: room.room_id, roomUid: room.room_uid,
      action: action === "close" ? "room.close" : "room.archive",
      ...(action === "close" ? {} : { archived: action === "archive" }) });
  }

  return { enabled, canChange: enabled && authorityReady, open, busy, pending, error, notice, rooms: managementRooms,
    show: () => { setOpen(true); void refresh(); }, close: () => setOpen(false),
    refresh, change, onRoomLifecycle: () => {
      if (busyRef.current) directoryInvalidatedRef.current = true;
      else void refresh();
    },
    retry: () => { if (pendingRef.current) void submit(pendingRef.current); },
  };
}
