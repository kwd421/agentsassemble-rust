import { useEffect, useId, useRef, useState } from "react";
import { X } from "../../ui/icons";
import { userFacingError } from "../../lib/apiErrors";

export default function CreateChannelModal({ onClose, onCreate, room = false, initialName = "" }: {
  room?: boolean;
  initialName?: string;
  onClose: () => void;
  onCreate: (params: { name: string; type: "text" }) => Promise<void | boolean>;
}) {
  const titleId = useId();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [opener] = useState(() => document.activeElement);
  const [name, setName] = useState(initialName);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const active = useRef(false);
  const submitting = useRef(false);
  const nameLimit = room ? 128 : 60;
  const validName = Boolean(name.trim()) && [...name.trim()].length <= nameLimit;
  useEffect(() => {
    active.current = true;
    const dialog = dialogRef.current;
    dialog?.showModal();
    inputRef.current?.focus();
    return () => {
      active.current = false;
      dialog?.close();
      if (opener instanceof HTMLElement && opener.isConnected) {
        const bounds = opener.getBoundingClientRect();
        if (bounds.right > 0 && bounds.left < window.innerWidth && bounds.bottom > 0 && bounds.top < window.innerHeight) opener.focus();
      }
    };
  }, [opener]);
  async function submit() {
    if (submitting.current) return;
    if (!validName) {
      setError(room ? "방 이름을 입력해 주세요." : "채널 이름을 입력해 주세요.");
      return;
    }
    submitting.current = true;
    setBusy(true); setError("");
    try {
      const created = await onCreate({ name: name.trim(), type: "text" });
      if (active.current) {
        if (created !== false) onClose();
        else {
          setError(room ? "방을 만들지 못했어요." : "채널을 만들지 못했어요.");
          setBusy(false);
        }
      }
    } catch (err) {
      if (active.current) {
        setError(userFacingError(err, room ? "방을 만들지 못했어요." : "채널을 만들지 못했어요."));
        setBusy(false);
      }
    } finally { submitting.current = false; }
  }
  return (
    <dialog ref={dialogRef} className="dc-create-channel-modal" aria-labelledby={titleId}
      style={{ position: "fixed", inset: 0, margin: "auto", maxHeight: "calc(100dvh - 32px)", overflowY: "auto", color: "var(--color-text-primary)" }}
      onCancel={(event) => { event.preventDefault(); if (!busy) onClose(); }}
      onClick={(event) => {
        if (event.target !== event.currentTarget || busy) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) onClose();
      }}>
      <header className="dc-create-channel-head">
        <h2 id={titleId}>{room ? "새 방 만들기" : "텍스트 채널 만들기"}</h2>
        <button type="button" className="dc-dialog-close" disabled={busy} onClick={onClose}
          aria-label="닫기"><X size={24} /></button>
      </header>
      <label className="dc-create-channel-name">
        {room ? "방 이름" : "채널 이름"}
        <input ref={inputRef} className="dc-input" autoFocus value={name} maxLength={nameLimit * 2}
          disabled={busy} placeholder={room ? "새 방" : "새 채널"}
          onChange={(event) => setName(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !event.nativeEvent.isComposing) {
              event.preventDefault(); void submit();
            }
          }} />
      </label>
      {[...name.trim()].length > nameLimit && <p role="alert">이름은 {nameLimit}자까지 입력할 수 있어요.</p>}
      {error && <p className="dc-channel-composer-error preserve-words" role="alert">{error}</p>}
      <div className="dc-create-channel-actions">
        <button type="button" className="dc-agent-create-secondary" disabled={busy} onClick={onClose} style={{ minHeight: 40 }}>취소</button>
        <button type="button" className="ops-cta" disabled={busy} aria-disabled={!validName || undefined} onClick={() => void submit()} style={{ padding: "0 16px" }}>
          {busy ? "만드는 중…" : "만들기"}
        </button>
      </div>
    </dialog>
  );
}
