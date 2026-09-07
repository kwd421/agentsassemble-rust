import { useState } from "react";
import type { useRoomLifecycle } from "../../app/useRoomLifecycle";
import type { ServerRoomDockSource } from "../../lib/roomDockModel";

export default function RoomManagementModal({ controller }: { controller: ReturnType<typeof useRoomLifecycle> }) {
  const [confirmation, setConfirmation] = useState<{ room: ServerRoomDockSource; action: "close" | "archive" | "restore" } | null>(null);
  const locked = controller.busy || Boolean(controller.pending) || !controller.canChange;
  return (
    <div className="dc-modal-backdrop" role="presentation">
      <section className="dc-modal" role="dialog" aria-modal="true" aria-label="방 관리">
        <header className="dc-modal-header"><h2>방 관리</h2><button type="button" onClick={controller.close}>닫기</button></header>
        <div className="dc-modal-body">
          <button type="button" disabled={controller.busy} onClick={() => void controller.refresh()}>목록 새로고침</button>
          {controller.error && <p role="alert">{controller.error}</p>}
          {controller.notice && <p role="status">{controller.notice}</p>}
          {controller.pending && <button type="button" disabled={controller.busy} onClick={controller.retry}>같은 요청 다시 확인</button>}
          {controller.rooms.map((room) => (
            <section key={room.room_uid}>
              <h3>{room.label}</h3>
              <p>{room.status === "archived" ? "보관됨" : room.status === "closed" ? "종료됨" : "활성"}{room.cleanup_pending ? " · 에이전트 정리 대기" : ""}</p>
              {room.status !== "closed" && <>
                <button type="button" disabled={locked || Boolean(room.cleanup_pending)} onClick={() => setConfirmation({ room, action: room.status === "archived" ? "restore" : "archive" })}>{room.status === "archived" ? "복원" : "보관"}</button>
                <button type="button" disabled={locked} onClick={() => setConfirmation({ room, action: "close" })}>방 종료</button>
              </>}
            </section>
          ))}
          {confirmation && <div>
            <p>{confirmation.room.label}: {confirmation.action === "close" ? "방을 종료할까요? 종료한 방은 다시 열 수 없습니다." : confirmation.action === "archive" ? "방을 보관할까요? 참가자 접속과 에이전트 실행이 종료됩니다." : "방을 복원할까요?"}</p>
            <button type="button" disabled={locked} onClick={() => { controller.change(confirmation.room, confirmation.action); setConfirmation(null); }}>확인</button>
            <button type="button" disabled={controller.busy} onClick={() => setConfirmation(null)}>취소</button>
          </div>}
        </div>
      </section>
    </div>
  );
}
