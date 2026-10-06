import { useEffect, useRef, useState, type ReactNode } from "react";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { X } from "lucide-react";
import type { CompanionInviteControls } from "../../app/useCompanionInvites";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import ProviderTileGrid from "./ProviderTileGrid";

export default function OwnComputerCreateModal({ roomLabel, providers, controls, onClose, locationChoice }: {
  roomLabel: string; providers: NativeCliProviderAvailability[]; controls: CompanionInviteControls;
  onClose: () => void; locationChoice?: ReactNode;
}) {
  const selectedProvider = providers.find((provider) => provider.id === controls.provider);
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
        <p className="dc-agent-hint preserve-words">모델과 작업 폴더는 이 컴퓨터에 뜨는 창에서 골라요. 내가 방을 나가면 이 AI도 함께 나가요.</p>
        <ProviderTileGrid providers={providers} selectedId={controls.provider} remoteCatalog disabled={controls.creating}
          onSelect={(provider) => { controls.setProvider(provider.id); controls.setDisplayName(provider.display_name); }} />
        {selectedProvider && <section className="dc-agent-section">
          <p className="dc-agent-section-title">기본 정보</p>
          <div className="dc-agent-field-grid"><label className="dc-agent-field"><span>표시 이름</span>
            <input maxLength={80} value={controls.displayName} disabled={controls.creating}
              onChange={(event) => controls.setDisplayName(event.target.value)} />
          </label></div>
        </section>}
        {controls.status && <p role="status" className="dc-agent-hint">{controls.status}</p>}
        {!isDesktopWebview() && controls.invites.map((invite) => <div key={invite.key} className="dc-invite-friend-row" style={{ flexWrap: "wrap" }}>
          <div className="dc-invite-card-copy"><strong>{invite.displayName}</strong><p>{invite.joined ? "방 참가가 확인됐어요. 실행 상태는 AI 목록에서 확인해 주세요." : !invite.copyable ? "만료됐거나 주소가 변경된 초대예요." : Date.parse(invite.expiresAt) - Date.now() < 120_000 ? "초대가 곧 만료돼요. 이 컴퓨터에서 설정을 마쳐 주세요." : null}</p></div>
          {(invite.copyable || invite.joined) && <a className="dc-invite-copy-button" style={{ minHeight: 44, display: "inline-flex", alignItems: "center" }} href={invite.nativeLink}>{invite.joined ? "실행 상태 열기" : "이 컴퓨터에서 설정하기"}</a>}
          <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }} disabled={!invite.copyable || invite.joined} onClick={() => void controls.copy(invite.key)}>참가 안내 복사</button>
        </div>)}
      </div>
      <footer className="dc-agent-create-footer">
        <div className="dc-agent-footer-actions">
          <button type="button" className="dc-agent-create-secondary" onClick={onClose}>취소</button>
          <button type="button" className="dc-agent-create-primary"
            disabled={controls.creating || !selectedProvider || !controls.displayName.trim()}
            onClick={() => void controls.create((link) => { window.location.assign(link); if (isDesktopWebview()) onClose(); })}>
            {controls.creating ? "여는 중…" : "이 컴퓨터에서 계속"}
          </button>
        </div>
      </footer>
    </dialog>
  </div>;
}
