import { useEffect, useId, useRef, useState } from "react";
import { X } from "lucide-react";

export default function CreateChannelModal({ onClose, onCreate }: {
  onClose: () => void;
  onCreate: (params: { name: string; type: "text" }) => Promise<void>;
}) {
  const titleId = useId();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const active = useRef(false);
  const submitting = useRef(false);
  const validName = Boolean(name.trim()) && [...name.trim()].length <= 60;
  useEffect(() => {
    active.current = true;
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => {
      active.current = false;
      dialog?.close();
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, [opener]);
  async function submit() {
    if (!validName || submitting.current) return;
    submitting.current = true;
    setBusy(true); setError("");
    try {
      await onCreate({ name: name.trim(), type: "text" });
      if (active.current) onClose();
    } catch (err) {
      if (active.current) {
        setError(err instanceof Error ? err.message : "채널을 만들지 못했어요.");
        setBusy(false);
      }
    } finally { submitting.current = false; }
  }
  return (
    <dialog ref={dialogRef} className="dc-create-channel-modal" aria-labelledby={titleId}
      style={{ position: "fixed", inset: 0, margin: "auto", width: "min(440px, calc(100vw - 32px))", maxHeight: "calc(100dvh - 32px)", overflowY: "auto", padding: 24, color: "var(--color-text-primary)" }}
      onCancel={(event) => { event.preventDefault(); if (!busy) onClose(); }}
      onClick={(event) => {
        if (event.target !== event.currentTarget || busy) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) onClose();
      }}>
      <header className="dc-create-channel-head">
        <h2 id={titleId}>텍스트 채널 만들기</h2>
        <button type="button" className="dc-settings-close" disabled={busy} onClick={onClose}
          aria-label="닫기" style={{ minWidth: 44, minHeight: 44 }}><X size={18} /></button>
      </header>
      <label className="dc-create-channel-name">
        채널 이름
        <input className="ops-input" autoFocus value={name} maxLength={120}
          disabled={busy} placeholder="구현방" style={{ minHeight: 44 }}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.nativeEvent.isComposing) {
              event.preventDefault(); void submit();
            }
          }} />
      </label>
      {[...name.trim()].length > 60 && <p role="alert">채널 이름은 60자까지 입력할 수 있어요.</p>}
      {error && <p className="dc-channel-composer-error preserve-words" role="alert">{error}</p>}
      <div className="dc-create-channel-actions">
        <button type="button" className="ops-button" disabled={busy} onClick={onClose} style={{ minHeight: 44 }}>취소</button>
        <button type="button" className="ops-cta" disabled={busy || !validName} onClick={() => void submit()} style={{ minHeight: 44 }}>
          {busy ? "만드는 중..." : "만들기"}
        </button>
      </div>
    </dialog>
  );
}
