import { useEffect, useRef, useState, type ReactNode } from "react";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { ArrowLeft, X } from "../../ui/icons";
import type { CompanionInviteControls } from "../../app/useCompanionInvites";
import type { NativeCliProviderAvailability } from "../../roomSocketClient";
import ProviderPicker from "./ProviderPicker";

export default function OwnComputerCreateModal({ roomLabel, providers, controls, onClose, locationChoice }: {
  roomLabel: string; providers: NativeCliProviderAvailability[]; controls: CompanionInviteControls;
  onClose: () => void; locationChoice?: ReactNode;
}) {
  const selectedProvider = providers.find((provider) => provider.id === controls.provider);
  // Same two steps as the server-agent dialog: the AI list, then its settings sliding in.
  // A preserved draft reopens on its settings.
  const [picking, setPicking] = useState(() => !selectedProvider);
  const [slide, setSlide] = useState<"forward" | "back" | null>(null);
  const showPicker = picking || !selectedProvider;
  const [displayNameEdited, setDisplayNameEdited] = useState(Boolean(controls.displayName));
  const [copiedKey, setCopiedKey] = useState("");
  const copiedTimer = useRef<number | undefined>(undefined);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  useEffect(() => {
    const dialog = dialogRef.current; dialog?.showModal();
    return () => { dialog?.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [opener]);
  useEffect(() => () => {
    if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
  }, []);
  return <div className="dc-modal-backdrop" role="presentation" onMouseDown={onClose}>
    <dialog ref={dialogRef} style={{ margin: "auto", color: "var(--color-text-primary)" }}
      onCancel={(event) => { event.preventDefault(); onClose(); }} className="dc-agent-create-modal" role="dialog" aria-modal="true" aria-label="에이전트 추가"
      data-provider-selected={!showPicker} onMouseDown={(event) => event.stopPropagation()}>
      <header className="dc-agent-create-head">
        {!showPicker && <button type="button" className="dc-agent-create-back" aria-label="AI 다시 고르기" title="AI 다시 고르기"
          disabled={controls.creating} onClick={() => { setPicking(true); setSlide("back"); }}><ArrowLeft size={18} /></button>}
        <div className="dc-agent-create-title"><p className="dc-agent-create-kicker preserve-words">{roomLabel}</p>
        <h2>에이전트 추가</h2></div><button type="button" onClick={onClose} aria-label="닫기"><X size={18} /></button></header>
      {showPicker ? <div className="dc-agent-create-body" key="pick" data-slide={slide === "back" ? "back" : undefined}>
        {locationChoice}
        <p className="dc-agent-hint preserve-words">모델과 작업 폴더는 이 컴퓨터에 뜨는 창에서 골라요. 내가 방을 나가면 이 AI도 함께 나가요.</p>
        <ProviderPicker providers={providers} selectedId={controls.provider} view="list" remoteCatalog disabled={controls.creating}
          onSelect={(provider) => {
            controls.setProvider(provider.id);
            if (!displayNameEdited) controls.setDisplayName(provider.display_name);
            setPicking(false); setSlide("forward");
          }} />
      </div> : <div className="dc-agent-create-body" key="settings" data-slide={slide === "forward" ? "forward" : undefined}>
        <ProviderPicker providers={providers} selectedId={controls.provider} view="card" remoteCatalog disabled={controls.creating}
          onSelect={(provider) => { controls.setProvider(provider.id); if (!displayNameEdited) controls.setDisplayName(provider.display_name); }} />
        {selectedProvider && <section className="dc-agent-section">
          <p className="dc-agent-section-title">기본 정보</p>
          <div className="dc-agent-field-grid"><label className="dc-agent-field"><span>표시 이름</span>
            <input maxLength={80} value={controls.displayName} disabled={controls.creating}
              onChange={(event) => { controls.setDisplayName(event.target.value); setDisplayNameEdited(true); }} />
          </label></div>
        </section>}
        {controls.status && <p role="status" className="dc-agent-hint">{controls.status}</p>}
        {!isDesktopWebview() && controls.invites.map((invite) => <div key={invite.key} className="dc-invite-friend-row" style={{ flexWrap: "wrap" }}>
          <div className="dc-invite-card-copy"><strong>{invite.displayName}</strong><p>{invite.joined ? "방 참가가 확인됐어요. 실행 상태는 AI 목록에서 확인해 주세요." : !invite.copyable ? "만료됐거나 주소가 변경된 초대예요." : Date.parse(invite.expiresAt) - Date.now() < 120_000 ? "초대가 곧 만료돼요. 이 컴퓨터에서 설정을 마쳐 주세요." : null}</p></div>
          {(invite.copyable || invite.joined) && <a className="dc-invite-copy-button" style={{ minHeight: 44, display: "inline-flex", alignItems: "center" }} href={invite.nativeLink}>{invite.joined ? "실행 상태 열기" : "이 컴퓨터에서 설정하기"}</a>}
          <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }} disabled={!invite.copyable || invite.joined} onClick={() => {
            void Promise.resolve(controls.copy(invite.key)).then((ok) => {
              if (ok !== true) return;
              setCopiedKey(invite.key);
              if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
              copiedTimer.current = window.setTimeout(() => setCopiedKey(""), 2000);
            });
          }}>{copiedKey === invite.key ? "복사됨" : "참가 안내 복사"}</button>
        </div>)}
      </div>}
      {!showPicker && <footer className="dc-agent-create-footer">
        <div className="dc-agent-footer-actions">
          <button type="button" className="dc-agent-create-secondary" onClick={onClose}>취소</button>
          <button type="button" className="dc-agent-create-primary"
            disabled={controls.creating || !selectedProvider || !controls.displayName.trim()}
            onClick={() => void controls.create((link) => { window.location.assign(link); if (isDesktopWebview()) onClose(); })}>
            {controls.creating ? "여는 중…" : "이 컴퓨터에서 계속"}
          </button>
        </div>
      </footer>}
    </dialog>
  </div>;
}
