import { useLayoutEffect, useRef, useState } from "react";
import { changeRoomLifecycle, type RoomLifecycleIntent } from "../api/roomLifecycle";
import { ApiError } from "../lib/apiErrors";
import type { RoomGuestSession } from "../lib/roomGuestSession";
import type { ServerRoomDockSource } from "../lib/roomDockModel";
import { createSecureRequestId } from "../lib/secureRequestId";
import type { Room } from "../types/generated/Room";

type Options = {
  enabled: boolean;
  session: RoomGuestSession | null;
  deviceToken: string;
  expired: boolean;
  room: Room | null;
  refreshProjection: () => void;
};
type Operation = { intent: RoomLifecycleIntent; sessionToken: string; deviceToken: string };

export function usePairedRoomLifecycle(options: Options) {
  const current = useRef(options);
  current.current = options;
  const [open, setOpen] = useState(false);
  const [room, setRoom] = useState<Room | null>(null);
  const [busy, setBusy] = useState(false);
  const [retirement, setRetirement] = useState<"none" | "ended" | "deleting">("none");
  const [pending, setPending] = useState<RoomLifecycleIntent | null>(null);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const owner = useRef<{ sessionToken: string; deviceToken: string } | null>(null);
  const operation = useRef<Operation | null>(null);
  const busyRef = useRef(false);
  useLayoutEffect(() => () => { operation.current = null; owner.current = null; }, []);
  const sameViewer = () => Boolean(owner.current && owner.current.deviceToken === options.deviceToken &&
    (owner.current.sessionToken === options.session?.sessionToken || options.expired));
  useLayoutEffect(() => {
    const previous = owner.current;
    if (!previous || (previous.deviceToken === options.deviceToken &&
      (previous.sessionToken === options.session?.sessionToken || options.expired))) return;
    operation.current = null;
    owner.current = null;
    busyRef.current = false;
    setBusy(false);
    setOpen(false);
    setRoom(null);
    setRetirement("none");
    setPending(null);
    setError("");
    setNotice("");
  }, [options.deviceToken, options.session?.sessionToken, options.expired]);

  async function submit(intent: RoomLifecycleIntent) {
    const session = current.current.session;
    if (!current.current.enabled || !session || busyRef.current || retirement !== "none") return;
    const captured: Operation = { intent, sessionToken: session.sessionToken, deviceToken: current.current.deviceToken };
    operation.current = captured;
    busyRef.current = true;
    setBusy(true);
    setPending(intent);
    setError("");
    setNotice("");
    const resultIsCurrent = () => operation.current === captured && current.current.deviceToken === captured.deviceToken &&
      (current.current.session?.sessionToken === captured.sessionToken || current.current.expired);
    let retainIntent = true;
    try {
      const result = await changeRoomLifecycle(intent, () => {
        if (!resultIsCurrent() || !current.current.enabled || current.current.room?.room_uid !== intent.roomUid) {
          throw new Error("현재 방 연결을 확인한 뒤 다시 시도해 주세요.");
        }
      }, { kind: "remote", sessionToken: captured.sessionToken, deviceToken: captured.deviceToken });
      if (!resultIsCurrent()) return;
      retainIntent = false;
      setPending(null);
      setRoom(result.room);
      setRetirement("ended");
      setNotice(result.deleted ? "방이 삭제됐습니다." : "방 상태가 변경되어 이 기기의 연결이 종료됐습니다.");
    } catch (failure) {
      if (!resultIsCurrent()) return;
      if (failure instanceof ApiError && failure.resolution === "rejected") {
        retainIntent = false;
        setPending(null);
      }
      if (failure instanceof ApiError && failure.code === "room_deletion_pending" && failure.resolution === "unresolved") {
        setRetirement("deleting");
        setNotice("방 삭제 요청이 접수되어 이 기기의 연결이 종료됐습니다. 삭제 완료 여부는 원래 앱에서 확인해 주세요.");
      } else {
        setError(`${failure instanceof Error ? failure.message : "방 관리 결과를 확인하지 못했습니다."} 연결이 종료됐다면 원래 앱에서 방 상태를 확인해 주세요.`);
      }
    } finally {
      if (resultIsCurrent()) {
        if (!retainIntent) operation.current = null;
        busyRef.current = false;
        setBusy(false);
      }
    }
  }

  function change(target: ServerRoomDockSource, action: "close" | "archive" | "restore" | "delete", confirmationName?: string) {
    const state = current.current;
    if (retirement !== "none" || !state.enabled || !state.session || operation.current || action === "restore" || !state.room ||
      target.room_id !== state.room.room_id || target.room_uid !== state.room.room_uid ||
      (action === "delete" && confirmationName !== state.room.label)) return;
    void submit({
      serverId: state.session.serverSurface.server_id,
      authorityLineageId: state.session.serverSurface.authority_lineage_id,
      requestId: createSecureRequestId(), roomId: state.room.room_id, roomUid: state.room.room_uid,
      action: action === "delete" ? "room.delete" : action === "close" ? "room.close" : "room.archive",
      ...(action === "archive" ? { archived: true } : {}), ...(action === "delete" ? { confirmationName } : {}),
    });
  }
  const displayedRoom = retirement === "none" && options.room?.room_uid === room?.room_uid ? options.room : room;
  const canChange = options.enabled && retirement === "none";
  return {
    enabled: canChange, canChange, open: open && sameViewer(), busy,
    pending: canChange ? pending : null, error, notice,
    rooms: displayedRoom ? [{ ...displayedRoom, deletion_pending: retirement === "deleting" }] : [], change,
    show: () => {
      if (!canChange || !options.session || !options.room) return;
      owner.current = { sessionToken: options.session.sessionToken, deviceToken: options.deviceToken };
      setRoom(options.room); setOpen(true);
    },
    close: () => setOpen(false),
    refresh: async () => {
      if (canChange) options.refreshProjection();
      else setNotice("이 기기의 방 연결이 종료됐습니다. 원래 앱에서 방 상태를 확인해 주세요.");
    },
    retry: () => { if (operation.current) void submit(operation.current.intent); },
    onRoomLifecycle: (next: Room) => {
      if (room?.room_uid === next.room_uid) {
        setRoom(next);
        setRetirement((previous) => previous === "deleting" ? previous : "ended");
      }
    },
  };
}
