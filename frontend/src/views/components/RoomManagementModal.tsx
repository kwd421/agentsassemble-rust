import { useEffect, useRef, useState } from "react";
import { MoreHorizontal, RefreshCw, X } from "lucide-react";
import type { useRoomLifecycle } from "../../app/useRoomLifecycle";
import type { ServerRoomDockSource } from "../../lib/roomDockModel";

type Action = "close" | "archive" | "restore" | "delete";

export default function RoomManagementModal({ controller }: { controller: ReturnType<typeof useRoomLifecycle> }) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [confirmation, setConfirmation] = useState<{ room: ServerRoomDockSource; action: Action } | null>(null);
  const [menu, setMenu] = useState<{ roomId: string; top: number; left: number } | null>(null);
  const [confirmationName, setConfirmationName] = useState("");
  const locked = controller.busy || Boolean(controller.pending) || !controller.canChange;
  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);

  function confirm(room: ServerRoomDockSource, action: Action) {
    setMenu(null);
    setConfirmationName("");
    setConfirmation({ room, action });
  }

  return (
    <div className="dc-modal-backdrop" role="presentation">
      <dialog ref={dialogRef} className="dc-invite-modal fixed inset-0 text-text-primary" style={{ margin: "auto" }} aria-modal="true" aria-label="방 관리"
        onClick={(event) => { if (!(event.target instanceof Element) || !event.target.closest("details")) setMenu(null); }}
        onCancel={(event) => { event.preventDefault(); if (controller.busy) return; if (menu) setMenu(null); else if (confirmation) setConfirmation(null); else controller.close(); }}>
        <header className="dc-create-channel-head" style={{ gap: 12 }}>
          <h2 className="font-semibold flex-1" style={{ fontSize: 18 }}>방 관리</h2>
          <button type="button" className="dc-settings-close" style={{ width: 44, height: 44 }} aria-label="목록 새로고침" disabled={controller.busy} onClick={() => void controller.refresh()}><RefreshCw size={18} /></button>
          <button type="button" className="dc-settings-close" style={{ width: 44, height: 44 }} aria-label="방 관리 닫기" onClick={controller.close}><X size={18} /></button>
        </header>
        <div className="grid gap-4">
          {controller.error && <p className="dc-channel-composer-error preserve-words" role="alert">{controller.error}</p>}
          {controller.notice && <p className="preserve-words text-text-muted" role="status">{controller.notice}</p>}
          {controller.pending && <button type="button" className="ops-button px-3 py-2" disabled={controller.busy} onClick={controller.retry}>같은 요청 다시 확인</button>}
          {!confirmation && controller.rooms.length === 0 && <p className="text-text-muted">관리할 방이 없습니다.</p>}
          {!confirmation && controller.rooms.map((room) => (
            <article className="dc-invite-card" key={room.room_uid}>
              <div className="flex items-center gap-3">
                <h3 className="min-w-0 flex-1 preserve-words">{room.label}</h3>
                <details open={menu?.roomId === room.room_id} className="relative" onKeyDown={(event) => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setMenu(null); } }}>
                  <summary className="dc-modal-close" style={{ width: 44, height: 44 }} aria-label={`${room.label} 관리 메뉴`} onClick={(event) => {
                    event.preventDefault(); const rect = event.currentTarget.getBoundingClientRect();
                    setMenu(menu?.roomId === room.room_id ? null : { roomId: room.room_id, top: Math.max(16, Math.min(rect.bottom, window.innerHeight - 160)), left: Math.max(16, Math.min(rect.right - 176, window.innerWidth - 192)) });
                  }}><MoreHorizontal size={18} /></summary>
                  {menu && menu.roomId === room.room_id && <div className="dc-member-context-menu" style={{ top: menu.top, left: menu.left }}>
                    {room.status === "active" && <button type="button" className="dc-member-context-menu-item" style={{ minHeight: 44 }} disabled={locked || Boolean(room.cleanup_pending)} onClick={() => confirm(room, "archive")}>보관</button>}
                    {room.status !== "closed" && <button type="button" className="dc-member-context-menu-item" data-variant="danger" style={{ minHeight: 44 }} disabled={locked} onClick={() => confirm(room, "close")}>방 종료</button>}
                    <button type="button" className="dc-member-context-menu-item" data-variant="danger" style={{ minHeight: 44 }} disabled={locked || Boolean(room.deletion_pending)} onClick={() => confirm(room, "delete")}>방 삭제</button>
                  </div>}
                </details>
              </div>
              <p className="text-text-muted">{room.deletion_pending ? "삭제 처리 중" : room.status === "archived" ? "보관됨" : room.status === "closed" ? "종료됨" : "활성"}{room.cleanup_pending ? " · 에이전트 정리 대기" : ""}</p>
              {room.status === "archived" && <button type="button" className="dc-agent-create-secondary" disabled={locked || Boolean(room.cleanup_pending)} onClick={() => confirm(room, "restore")}>복원</button>}
            </article>
          ))}
          {confirmation && <section className="dc-invite-card" aria-label="방 상태 변경 확인">
            <p className="preserve-words">{confirmation.room.label}: {confirmation.action === "delete" ? "방과 대화·첨부 데이터를 영구 삭제합니다. 계속하려면 현재 방 이름을 정확히 입력하세요." : confirmation.action === "close" ? "방을 종료할까요? 종료한 방은 다시 열 수 없습니다." : confirmation.action === "archive" ? "방을 보관할까요? 참가자 접속과 에이전트 실행이 종료됩니다." : "방을 복원할까요?"}</p>
            {confirmation.action === "delete" && <label className="dc-agent-field">현재 방 이름
              <input autoFocus value={confirmationName} disabled={locked} autoComplete="off" onChange={(event) => setConfirmationName(event.target.value)} />
            </label>}
            <div className="dc-create-channel-actions">
              <button type="button" autoFocus={confirmation.action !== "delete"} className="dc-agent-create-secondary" style={{ minHeight: 44 }} disabled={controller.busy} onClick={() => setConfirmation(null)}>취소</button>
              <button type="button" className={confirmation.action === "delete" || confirmation.action === "close" ? "dc-member-session-button" : "dc-agent-create-primary"} data-variant={confirmation.action === "delete" || confirmation.action === "close" ? "danger" : undefined} style={{ minHeight: 44 }} disabled={locked || (confirmation.action === "delete" && confirmationName !== confirmation.room.label)} onClick={() => {
                if (confirmation.action === "delete") controller.change(confirmation.room, "delete", confirmationName);
                else controller.change(confirmation.room, confirmation.action);
                setConfirmation(null);
              }}>{confirmation.action === "delete" ? "방 삭제" : confirmation.action === "close" ? "방 종료" : confirmation.action === "archive" ? "보관" : "복원"}</button>
            </div>
          </section>}
        </div>
      </dialog>
    </div>
  );
}
