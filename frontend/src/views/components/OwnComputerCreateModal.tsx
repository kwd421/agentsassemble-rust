import { useEffect, useRef, useState, type ReactNode } from "react";
import { X } from "lucide-react";
import type { CompanionInviteControls } from "../../app/useCompanionInvites";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import CompanionInviteCard from "./CompanionInviteCard";

export default function OwnComputerCreateModal({ roomLabel, providers, controls, onClose, locationChoice }: {
  roomLabel: string; providers: NativeCliProviderAvailability[]; controls: CompanionInviteControls;
  onClose: () => void; locationChoice?: ReactNode;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  useEffect(() => {
    const dialog = dialogRef.current; dialog?.showModal();
    return () => { dialog?.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [opener]);
  return <div className="dc-modal-backdrop" role="presentation" onMouseDown={onClose}>
    <dialog ref={dialogRef} style={{ margin: "auto", color: "var(--color-text-primary)" }}
      onCancel={(event) => { event.preventDefault(); onClose(); }} className="dc-agent-create-modal" role="dialog" aria-modal="true" aria-label="에이전트 추가"
      onMouseDown={(event) => event.stopPropagation()}>
      <header className="dc-agent-create-head"><div><p className="dc-agent-create-kicker preserve-words">{roomLabel}</p>
        <h2>에이전트 추가</h2></div><button type="button" onClick={onClose} aria-label="닫기"><X size={18} /></button></header>
      <div className="dc-agent-create-body">
        {locationChoice}
        <p className="dc-agent-hint preserve-words">이 컴퓨터에서 AI를 실행해요. 앱에서 모델과 작업 폴더를 선택해 주세요.</p>
        <CompanionInviteCard controls={controls} providers={providers} />
      </div>
      <footer className="dc-agent-create-footer">
        <button type="button" className="dc-agent-create-secondary" onClick={onClose}>닫기</button></footer>
    </dialog>
  </div>;
}
