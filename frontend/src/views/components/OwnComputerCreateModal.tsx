import { useEffect, useRef, useState } from "react";
import { X } from "lucide-react";
import type { CompanionInviteControls } from "../../app/useCompanionInvites";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import CompanionInviteCard from "./CompanionInviteCard";

export default function OwnComputerCreateModal({ roomLabel, providers, controls, onClose, onHost }: {
  roomLabel: string; providers: NativeCliProviderAvailability[]; controls: CompanionInviteControls;
  onClose: () => void; onHost: () => void;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  useEffect(() => {
    const dialog = dialogRef.current; dialog?.showModal();
    return () => { dialog?.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [opener]);
  return <div className="dc-modal-backdrop" role="presentation" onMouseDown={onClose}>
    <dialog ref={dialogRef} style={{ margin: "auto", color: "var(--color-text-primary)" }}
      onCancel={(event) => { event.preventDefault(); onClose(); }} className="dc-agent-create-modal" role="dialog" aria-modal="true" aria-label="내 PC에서 에이전트 추가"
      onMouseDown={(event) => event.stopPropagation()}>
      <header className="dc-agent-create-head"><div><p className="dc-agent-create-kicker preserve-words">{roomLabel}</p>
        <h2>내 PC에서 에이전트 추가</h2></div><button type="button" onClick={onClose} aria-label="닫기"><X size={18} /></button></header>
      <div className="dc-agent-create-body">
        <p className="dc-agent-hint preserve-words">내 PC의 AgentsAssemble 앱에서 로그인하고 모델·작업 폴더를 선택해 이 방에 추가해요.</p>
        <CompanionInviteCard controls={controls} providers={providers} />
      </div>
      <footer className="dc-agent-create-footer"><button type="button" className="dc-agent-create-secondary" onClick={onHost}>방이 열린 PC에서 추가</button>
        <button type="button" className="dc-agent-create-secondary" onClick={onClose}>닫기</button></footer>
    </dialog>
  </div>;
}
