import { useEffect, useId, useRef, useState } from "react";
import { LogOut, X } from "lucide-react";
import { userFacingError } from "../../lib/apiErrors";

export default function LeaveRoomDialog({
  roomLabel,
  pairedDevice: initialPairedDevice = false,
  onClose,
  onConfirm,
}: {
  roomLabel: string;
  pairedDevice?: boolean;
  onClose: () => void;
  onConfirm: () => Promise<void>;
}) {
  const titleId = useId();
  const [pairedDevice] = useState(initialPairedDevice);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");

  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => {
      dialog?.close();
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [opener]);

  async function confirmLeave() {
    if (busy) return;
    setBusy(true);
    setError("");
    try {
      await onConfirm();
      onClose();
    } catch (errorValue) {
      setError(
        userFacingError(errorValue, "방에서 나가지 못했어요.")
      );
      setBusy(false);
    }
  }

  return (
    <dialog
      ref={dialogRef}
      className="dc-create-channel-modal"
      aria-labelledby={titleId}
      style={{ position: "fixed", inset: 0, margin: "auto", maxHeight: "calc(100dvh - 32px)", overflowY: "auto", color: "var(--color-text-primary)" }}
      onCancel={(event) => { event.preventDefault(); if (!busy) onClose(); }}
      onClick={(event) => { if (event.target === event.currentTarget && !busy) onClose(); }}
    >
        <header className="dc-create-channel-head">
          <h2 id={titleId} className="preserve-words">‘{roomLabel}’{pairedDevice ? " 기기 연결을 해제할까요?" : "에서 나갈까요?"}</h2>
          <button
            type="button"
            className="dc-dialog-close"
            onClick={onClose}
            disabled={busy}
            aria-label="방 나가기 취소"
          >
            <X size={24} />
          </button>
        </header>

        <div className="grid gap-2 text-[14px] leading-6 text-text-muted">
          <p className="preserve-words">
            {pairedDevice ? "이 기기를 다시 연결하려면 서버 컴퓨터에서 새 연결 링크를 받아야 해요." : "나간 뒤 다시 들어오려면 새 초대 링크가 필요해요."}
          </p>
          <p className="preserve-words font-bold text-text-primary">
            {pairedDevice ? "이 기기의 방 접속만 끝나요. 방 주인과 에이전트는 계속 참여해요." : "내 에이전트도 함께 나가고, 실행 중인 에이전트는 멈춰요."}
          </p>
          <p className="preserve-words">
            기존 대화 기록은 서버에 남아요.
          </p>
        </div>

        {error && (
          <p className="dc-channel-composer-error preserve-words" role="alert">
            {error}
          </p>
        )}

        <div className="dc-create-channel-actions">
          <button
            autoFocus
            type="button"
            className="dc-agent-create-secondary"
            style={{ minHeight: 40 }}
            onClick={onClose}
            disabled={busy}
          >
            취소
          </button>
          <button
            type="button"
            className="dc-agent-create-primary dc-leave-room-confirm"
            style={{ minHeight: 40 }}
            onClick={() => void confirmLeave()}
            disabled={busy}
          >
            <LogOut size={16} />
            {busy ? "나가는 중…" : pairedDevice ? "연결 해제" : "방 나가기"}
          </button>
        </div>
    </dialog>
  );
}
