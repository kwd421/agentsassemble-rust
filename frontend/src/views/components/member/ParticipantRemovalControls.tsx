import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { MoreHorizontal } from "lucide-react";

export type ParticipantRemovalAction = (participantId: string, action: "kick" | "export") => Promise<void>;

export default function ParticipantRemovalControls({ participantId, displayName, onRemove, showMenuTrigger = false }: {
  showMenuTrigger?: boolean;
  participantId: string;
  displayName: string;
  onRemove: ParticipantRemovalAction;
}) {
  const [menuPosition, setMenuPosition] = useState({ top: 0, left: 0 });
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDetailsElement>(null);
  const actionTriggerRef = useRef<HTMLButtonElement | null>(null);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const busyRef = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [confirmAction, setConfirmAction] = useState<"kick" | "export" | null>(null);

  useEffect(() => {
    if (!confirmAction) return;
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => { dialog?.close(); actionTriggerRef.current?.focus(); };
  }, [confirmAction]);

  useEffect(() => {
    if (!menuOpen || confirmAction) return;
    const dismiss = (event: PointerEvent) => {
      if (event.target instanceof Node && !menuRef.current?.contains(event.target)) setMenuOpen(false);
    };
    document.addEventListener("pointerdown", dismiss);
    return () => document.removeEventListener("pointerdown", dismiss);
  }, [menuOpen, confirmAction]);

  async function remove(action: "kick" | "export") {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    try {
      await onRemove(participantId, action);
      setConfirmAction(null);
      setMenuOpen(false);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "참가자를 제거하지 못했습니다.");
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  const actions = (
    <div onClick={(event) => event.stopPropagation()} onKeyDown={(event) => { if (event.key !== "Escape") event.stopPropagation(); }}>
      <button type="button" className="dc-member-context-menu-item" data-variant="danger" style={{ minHeight: 44 }} disabled={busy}
        aria-label={`${displayName} 강퇴`} onClick={(event) => { actionTriggerRef.current = event.currentTarget; setConfirmAction("kick"); }}>
        강퇴
      </button>
      <button type="button" className="dc-member-context-menu-item" data-variant="danger" style={{ minHeight: 44 }} disabled={busy}
        aria-label={`${displayName} 참가 종료`} title="참가를 종료합니다. 에이전트는 다시 시작할 수 없습니다."
        onClick={(event) => { actionTriggerRef.current = event.currentTarget; setConfirmAction("export"); }}>
        참가 종료
      </button>
      {confirmAction && createPortal(
        <dialog ref={dialogRef} className="dc-member-detail-modal fixed inset-0 text-text-primary" style={{ margin: "auto" }}
          aria-label={confirmAction === "kick" ? "참가자 강퇴 확인" : "참가 종료 확인"}
          onCancel={(event) => { event.preventDefault(); event.stopPropagation(); if (!busy) { setConfirmAction(null); setError(""); } }}>
          <h2 className="font-semibold" style={{ fontSize: 18 }}>{confirmAction === "kick" ? "참가자를 내보낼까요?" : "참가를 종료할까요?"}</h2>
          <p className="preserve-words" style={{ margin: "20px 0" }}>{displayName}{confirmAction === "kick" ? "의 방 접속과 실행이 종료됩니다. 나중에 다시 참가시킬 수 있어요." : "의 참가와 실행이 종료됩니다. 이 에이전트 세션은 다시 시작할 수 없습니다."}</p>
          {error && <p className="dc-channel-composer-error preserve-words" role="alert">{error}</p>}
          <div className="dc-create-channel-actions">
            <button type="button" autoFocus className="dc-agent-create-secondary" style={{ minHeight: 44 }} disabled={busy} onClick={() => { setConfirmAction(null); setError(""); }}>취소</button>
            <button type="button" className="dc-member-session-button" data-variant="danger" style={{ minHeight: 44 }} disabled={busy} onClick={() => void remove(confirmAction)}>{busy ? "처리 중…" : confirmAction === "kick" ? "강퇴" : "참가 종료"}</button>
          </div>
        </dialog>, document.body
      )}
    </div>
  );
  if (!showMenuTrigger) return actions;
  return <details ref={menuRef} open={menuOpen} className="relative" onClick={(event) => event.stopPropagation()}
    onKeyDown={(event) => { event.stopPropagation(); if (event.key === "Escape" && !busy && !confirmAction) setMenuOpen(false); }}>
    <summary className="dc-modal-close" style={{ width: 44, height: 44 }} aria-label={`${displayName} 관리 메뉴`} onClick={(event) => {
      event.preventDefault(); if (busy) return;
      const rect = event.currentTarget.getBoundingClientRect();
      setMenuPosition({ top: Math.max(16, Math.min(rect.bottom, window.innerHeight - 180)), left: Math.max(16, Math.min(rect.right - 240, window.innerWidth - 256)) });
      setMenuOpen(!menuOpen); setConfirmAction(null); setError("");
    }}><MoreHorizontal size={18} /></summary>
    {menuOpen && <div className="dc-member-context-menu" style={menuPosition}>
      {actions}
    </div>}
  </details>;
}
