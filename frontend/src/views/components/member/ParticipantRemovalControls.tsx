import { useRef, useState } from "react";

export type ParticipantRemovalAction = (participantId: string, action: "kick" | "export") => Promise<void>;

export default function ParticipantRemovalControls({ participantId, displayName, onRemove }: {
  participantId: string;
  displayName: string;
  onRemove: ParticipantRemovalAction;
}) {
  const busyRef = useRef(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [confirmAction, setConfirmAction] = useState<"kick" | "export" | null>(null);

  async function remove(action: "kick" | "export") {
    if (busyRef.current) return;
    busyRef.current = true;
    setBusy(true);
    setError("");
    try {
      await onRemove(participantId, action);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "참가자를 제거하지 못했습니다.");
    } finally {
      busyRef.current = false;
      setBusy(false);
    }
  }

  return (
    <div onClick={(event) => event.stopPropagation()} onKeyDown={(event) => event.stopPropagation()}>
      <button type="button" className="dc-member-context-menu-item" disabled={busy}
        aria-label={`${displayName} 강퇴`} onClick={() => setConfirmAction("kick")}>
        강퇴
      </button>
      <button type="button" className="dc-member-context-menu-item" disabled={busy}
        aria-label={`${displayName} 참가 종료`} title="참가를 종료합니다. 에이전트는 다시 시작할 수 없습니다."
        onClick={() => setConfirmAction("export")}>
        참가 종료
      </button>
      {confirmAction && (
        <div>
          <p>{displayName}{confirmAction === "kick" ? "을(를) 방에서 내보낼까요?" : "의 참가를 종료할까요? 에이전트는 다시 시작할 수 없습니다."}</p>
          <button type="button" disabled={busy} onClick={() => void remove(confirmAction)}>확인</button>
          <button type="button" disabled={busy} onClick={() => { setConfirmAction(null); setError(""); }}>취소</button>
        </div>
      )}
      {error && <p role="alert">{error}</p>}
    </div>
  );
}
