import { useEffect, useRef, useState } from "react";
import type { useRoomLifecycle } from "../../../app/useRoomLifecycle";

export type RoomLifecycleController = Pick<ReturnType<typeof useRoomLifecycle>,
  "busy" | "pending" | "canChange" | "refresh" | "rooms" | "error" | "notice" | "retry" | "change">;

export type RoomDeleteTarget = { roomId: string; roomUid?: string; label: string };

/**
 * Deleting a room is the one irreversible action the app offers, so it follows the
 * pattern people already know from Discord: its own modal, the room's exact name
 * typed back, and a red confirm.
 */
export default function RoomDeleteDialog({
  target,
  controller,
  onClose,
}: {
  target: RoomDeleteTarget;
  controller: RoomLifecycleController;
  onClose: () => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  const [confirmationName, setConfirmationName] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const { refresh } = controller;

  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => { dialog?.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [opener]);

  // The controller only accepts a room it has listed, so the listing comes first.
  useEffect(() => { void refresh(); }, [refresh]);

  const room = controller.rooms.find(
    (candidate) => candidate.room_id === target.roomId && (!target.roomUid || candidate.room_uid === target.roomUid)
  );
  const deleted = submitted && !controller.pending && !controller.busy && Boolean(controller.notice);
  const locked = controller.busy || Boolean(controller.pending) || !controller.canChange || !room;

  return (
    <div className="dc-modal-backdrop" role="presentation" onClick={() => { if (!controller.busy) onClose(); }}>
      <dialog
        ref={dialogRef}
        className="dc-room-delete"
        role="alertdialog"
        aria-labelledby="room-delete-title"
        onCancel={(event) => { event.preventDefault(); event.stopPropagation(); if (!controller.busy) onClose(); }}
        onClick={(event) => { event.stopPropagation(); if (event.target === event.currentTarget && !controller.busy) onClose(); }}
      >
        <h2 id="room-delete-title" className="preserve-words">‘{target.label}’ 삭제</h2>
        {deleted ? (
          <p className="preserve-words" role="status">{controller.notice}</p>
        ) : (
          <p className="preserve-words">
            정말 <strong>{target.label}</strong>을(를) 삭제할까요? 이 방의 대화와 첨부 파일, 에이전트 설정이 모두
            사라지고 <strong>되돌릴 수 없어요.</strong>
          </p>
        )}
        {!deleted && (
          <label className="dc-room-delete-field">
            <span>방 이름 입력</span>
            <input autoFocus value={confirmationName} disabled={locked} autoComplete="off"
              onChange={(event) => setConfirmationName(event.target.value)} />
          </label>
        )}
        {!room && !controller.error && (
          <p className="dc-room-delete-note" role="status">
            {controller.busy ? "방 정보를 확인하는 중이에요." : "이 방의 관리 정보를 확인하지 못했어요."}
          </p>
        )}
        {controller.error && <p className="dc-channel-composer-error preserve-words" role="alert">{controller.error}</p>}
        {controller.pending && (
          <button type="button" className="ops-button px-3 py-2" disabled={controller.busy} onClick={controller.retry}>
            같은 요청 다시 확인
          </button>
        )}
        <div className="dc-room-delete-actions">
          <button type="button" className="dc-room-delete-cancel" style={{ minHeight: 44 }}
            disabled={controller.busy} onClick={onClose}>
            {deleted ? "닫기" : "취소"}
          </button>
          {!deleted && (
            <button
              type="button"
              className="dc-room-delete-confirm"
              style={{ minHeight: 44 }}
              disabled={locked || confirmationName !== target.label}
              onClick={() => {
                if (!room) return;
                setSubmitted(true);
                controller.change(room, "delete", confirmationName);
              }}
            >
              방 삭제
            </button>
          )}
        </div>
      </dialog>
    </div>
  );
}
