import { useEffect, useState } from "react";
import { RefreshCw } from "lucide-react";
import type { ServerRoomDockSource } from "../../../lib/roomDockModel";
import RoomLifecycleConfirm, {
  type RoomLifecycleAction,
  type RoomLifecycleController,
  type RoomLifecycleTarget,
} from "./RoomLifecycleConfirm";

function roomStatusLabel(room: ServerRoomDockSource) {
  const status = String(room.status || "active").toLowerCase();
  const state = room.deletion_pending ? "삭제 처리 중" : status === "closed" ? "종료됨" : status === "archived" || room.archived ? "보관됨" : "활성";
  return room.cleanup_pending ? `${state} · 에이전트 정리 대기` : state;
}

export function inactiveRooms(rooms: readonly ServerRoomDockSource[]) {
  return rooms.filter((room) => {
    const status = String(room.status || "active").toLowerCase();
    return Boolean(room.archived) || status === "archived" || status === "closed";
  });
}

/**
 * Rooms the rail cannot show. An archived or closed room is filtered out of the
 * dock, so this list is the only way back to one.
 */
export default function ArchivedRoomList({ controller }: { controller: RoomLifecycleController }) {
  const [confirming, setConfirming] = useState<{ target: RoomLifecycleTarget; action: RoomLifecycleAction } | null>(null);
  const { refresh } = controller;
  useEffect(() => { void refresh(); }, [refresh]);

  const rooms = inactiveRooms(controller.rooms);
  const locked = controller.busy || Boolean(controller.pending) || !controller.canChange;

  function confirm(room: ServerRoomDockSource, action: RoomLifecycleAction) {
    setConfirming({
      target: { roomId: room.room_id, roomUid: room.room_uid, label: String(room.label || room.room_id) },
      action,
    });
  }

  return (
    <div className="dc-archived-rooms">
      <div className="dc-archived-rooms-head">
        <p className="dc-settings-field-label">보관·종료된 방</p>
        <button type="button" className="dc-upload-button" disabled={controller.busy}
          onClick={() => void refresh()} aria-label="보관된 방 목록 새로고침">
          <RefreshCw size={14} />
          새로고침
        </button>
      </div>
      {controller.error && <p className="dc-channel-composer-error preserve-words" role="alert">{controller.error}</p>}
      {rooms.length === 0 ? (
        <p className="text-text-muted preserve-words">
          {controller.busy ? "방 목록을 불러오는 중이에요." : "보관하거나 종료한 방이 없어요."}
        </p>
      ) : (
        <ul className="dc-archived-room-rows" aria-label="보관·종료된 방">
          {rooms.map((room) => (
            <li className="dc-archived-room-row" key={room.room_uid || room.room_id}>
              <span className="min-w-0 flex-1">
                <span className="dc-invite-friend-name preserve-words">{room.label || room.room_id}</span>
                <span className="dc-invite-friend-handle preserve-words">{roomStatusLabel(room)}</span>
              </span>
              {String(room.status || "").toLowerCase() === "archived" || room.archived ? (
                <button type="button" className="dc-invite-copy-button" style={{ minHeight: 40 }}
                  disabled={locked || Boolean(room.cleanup_pending) || Boolean(room.deletion_pending)}
                  onClick={() => confirm(room, "restore")}>복원</button>
              ) : null}
              <button type="button" className="dc-member-session-button" data-variant="danger" style={{ minHeight: 40 }}
                disabled={locked || Boolean(room.deletion_pending)}
                onClick={() => confirm(room, "delete")}>방 삭제</button>
            </li>
          ))}
        </ul>
      )}
      {confirming && (
        <RoomLifecycleConfirm target={confirming.target} action={confirming.action} controller={controller}
          onClose={() => setConfirming(null)} />
      )}
    </div>
  );
}
