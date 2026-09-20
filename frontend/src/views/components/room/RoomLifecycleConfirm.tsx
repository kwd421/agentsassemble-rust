import { useEffect, useRef, useState } from "react";
import type { useRoomLifecycle } from "../../../app/useRoomLifecycle";

export type RoomLifecycleAction = "close" | "archive" | "restore" | "delete";

export type RoomLifecycleController = Pick<ReturnType<typeof useRoomLifecycle>,
  "busy" | "pending" | "canChange" | "refresh" | "rooms" | "error" | "notice" | "retry" | "change">;

export type RoomLifecycleTarget = { roomId: string; roomUid?: string; label: string };

export const ROOM_LIFECYCLE_LABELS: Record<RoomLifecycleAction, string> = {
  archive: "보관",
  close: "방 종료",
  delete: "방 삭제",
  restore: "복원",
};

const QUESTIONS: Record<RoomLifecycleAction, string> = {
  archive: "방을 보관할까요? 참가자 접속과 에이전트 실행이 종료됩니다. 보관한 방은 서버 설정의 방 관리에서 복원할 수 있어요.",
  close: "방을 종료할까요? 종료한 방은 다시 열 수 없습니다.",
  delete: "방과 대화·첨부 데이터를 영구 삭제합니다. 계속하려면 현재 방 이름을 정확히 입력하세요.",
  restore: "방을 복원할까요?",
};

/**
 * Confirms one room's lifecycle change. The controller only accepts rooms it has
 * listed, so the dialog refreshes on mount and waits for that listing before it
 * enables the action.
 */
export default function RoomLifecycleConfirm({
  target,
  action,
  controller,
  onClose,
}: {
  target: RoomLifecycleTarget;
  action: RoomLifecycleAction;
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

  useEffect(() => { void refresh(); }, [refresh]);

  const room = controller.rooms.find(
    (candidate) => candidate.room_id === target.roomId && (!target.roomUid || candidate.room_uid === target.roomUid)
  );
  const settled = submitted && !controller.pending && !controller.busy && Boolean(controller.notice);
  const locked = controller.busy || Boolean(controller.pending) || !controller.canChange || !room;
  const nameMismatch = action === "delete" && confirmationName !== target.label;
  const destructive = action === "delete" || action === "close";

  return (
    <dialog
      ref={dialogRef}
      className="dc-invite-confirm"
      role="alertdialog"
      aria-labelledby="room-lifecycle-title"
      style={{ margin: "auto", width: "min(430px, calc(100vw - 32px))", maxHeight: "calc(100dvh - 32px)", overflowY: "auto", padding: 24 }}
      onCancel={(event) => { event.preventDefault(); event.stopPropagation(); if (!controller.busy) onClose(); }}
      onClick={(event) => { event.stopPropagation(); if (event.target === event.currentTarget && !controller.busy) onClose(); }}
    >
      <h3 id="room-lifecycle-title" className="preserve-words">
        {target.label} · {ROOM_LIFECYCLE_LABELS[action]}
      </h3>
      {settled ? (
        <p className="preserve-words" role="status">{controller.notice}</p>
      ) : (
        <p className="preserve-words">{QUESTIONS[action]}</p>
      )}
      {!room && !controller.error && (
        <p className="preserve-words" role="status">
          {controller.busy ? "방 정보를 확인하는 중이에요." : "이 방의 관리 정보를 확인하지 못했어요."}
        </p>
      )}
      {action === "delete" && !settled && (
        <label className="dc-agent-field">현재 방 이름
          <input autoFocus value={confirmationName} disabled={locked} autoComplete="off"
            onChange={(event) => setConfirmationName(event.target.value)} />
        </label>
      )}
      {controller.error && <p className="dc-channel-composer-error preserve-words" role="alert">{controller.error}</p>}
      {controller.pending && (
        <button type="button" className="ops-button px-3 py-2" disabled={controller.busy} onClick={controller.retry}>
          같은 요청 다시 확인
        </button>
      )}
      <div className="dc-invite-confirm-actions">
        <button type="button" className="dc-agent-create-secondary" style={{ minWidth: 44, minHeight: 44 }}
          autoFocus={action !== "delete"} disabled={controller.busy} onClick={onClose}>
          {settled ? "닫기" : "취소"}
        </button>
        {!settled && (
          <button
            type="button"
            className={destructive ? "dc-member-session-button" : "dc-invite-confirm-primary"}
            data-variant={destructive ? "danger" : undefined}
            style={{ minWidth: 44, minHeight: 44 }}
            disabled={locked || nameMismatch}
            onClick={() => {
              if (!room) return;
              setSubmitted(true);
              if (action === "delete") controller.change(room, "delete", confirmationName);
              else controller.change(room, action);
            }}
          >
            {ROOM_LIFECYCLE_LABELS[action]}
          </button>
        )}
      </div>
    </dialog>
  );
}
