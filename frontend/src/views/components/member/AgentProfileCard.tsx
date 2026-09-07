import { useEffect, useRef, useState, type ReactNode } from "react";
import { Camera, MoreHorizontal, Pencil, X } from "lucide-react";
import { AGENT_PROFILE_NAME_CHARACTER_LIMIT } from "../../../types/generated/AGENT_PROFILE_WIRE";
import type { RoomAgentSession } from "../../../api";
import ImageCropper from "../ImageCropper";
import ProviderLogo from "../ProviderLogo";

export default function AgentProfileCard({ session, avatarImage, detail, onClose, onSave, onAvatarUpdate, children }: {
  session: RoomAgentSession;
  avatarImage?: string;
  detail?: string;
  onClose?: () => void;
  onSave?: (session: RoomAgentSession, settings: Record<string, string>) => void | Promise<void>;
  onAvatarUpdate?: (session: RoomAgentSession, file: File, displayName: string, signal: AbortSignal) => Promise<void>;
  children: ReactNode;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const uploadRef = useRef<AbortController | null>(null);
  const [editing, setEditing] = useState(false);
  const [name, setName] = useState(session.display_name);
  const [photo, setPhoto] = useState<File | null>(null);
  const [photoPreview, setPhotoPreview] = useState("");
  const [clearPhoto, setClearPhoto] = useState(false);
  const [cropFile, setCropFile] = useState<File | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  useEffect(() => () => uploadRef.current?.abort(), []);
  useEffect(() => {
    if (!photo) { setPhotoPreview(""); return; }
    const url = URL.createObjectURL(photo);
    setPhotoPreview(url);
    return () => URL.revokeObjectURL(url);
  }, [photo]);
  const canEdit = Boolean(onSave);
  useEffect(() => {
    if (canEdit) return;
    uploadRef.current?.abort();
    setEditing(false);
    setBusy(false);
    setPhoto(null);
    setCropFile(null);
  }, [canEdit]);

  function beginEditing() {
    setName(session.display_name);
    setPhoto(null);
    setClearPhoto(false);
    setCropFile(null);
    setError("");
    setNotice("");
    setEditing(true);
  }

  function cancelEditing() {
    if (busy) return;
    setEditing(false);
    setPhoto(null);
    setCropFile(null);
    setError("");
  }

  const nameValid = name.trim().length > 0 && Array.from(name.trim()).length <= AGENT_PROFILE_NAME_CHARACTER_LIMIT;
  const changed = name.trim() !== session.display_name || Boolean(photo) || (clearPhoto && Boolean(session.avatar_image_url));
  const preview = photoPreview || (clearPhoto ? "" : avatarImage);

  async function save() {
    if (!onSave || busy || !nameValid || !changed || cropFile) return;
    setBusy(true);
    setError("");
    const controller = new AbortController();
    uploadRef.current = controller;
    try {
      if (photo) {
        if (!onAvatarUpdate) throw new Error("사진 변경을 사용할 수 없습니다.");
        await onAvatarUpdate(session, photo, name, controller.signal);
      } else {
        await onSave(session, { display_name: name, ...(clearPhoto ? { avatar_image_url: "" } : {}) });
      }
      if (!controller.signal.aborted) {
        setEditing(false);
        setPhoto(null);
        setNotice("프로필을 변경했어요.");
      }
    } catch (failure) {
      if (!controller.signal.aborted) setError(failure instanceof Error ? failure.message : "프로필을 저장하지 못했어요. 다시 시도해 주세요.");
    } finally {
      if (!controller.signal.aborted) setBusy(false);
      if (uploadRef.current === controller) uploadRef.current = null;
    }
  }

  const editor = (
    <form aria-label="에이전트 프로필 편집" onSubmit={(event) => { event.preventDefault(); void save(); }}
      onKeyDown={(event) => { if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); cancelEditing(); } }}>
      <header className="dc-member-detail-modal-head">
        <h2 className="min-w-0 flex-1">{cropFile ? "사진 위치 조정" : "프로필 편집"}</h2>
        <button type="button" className="dc-modal-close" aria-label="프로필 편집 취소" disabled={busy} onClick={cancelEditing}><X size={18} /></button>
      </header>
      {cropFile ? <ImageCropper file={cropFile} onCancel={() => setCropFile(null)} onCropped={(file) => {
        setPhoto(file); setClearPhoto(false); setCropFile(null);
      }} /> : <>
        <div className="flex items-center gap-3" style={{ margin: "24px 0" }}>
          <button type="button" className="dc-member-detail-modal-avatar relative" style={{ width: 80, height: 80 }}
            aria-label="프로필 사진 변경" title="프로필 사진 변경" disabled={busy || !onAvatarUpdate}
            onClick={() => inputRef.current?.click()}>
            {preview ? <img className="dc-member-avatar-image" src={preview} alt="프로필 사진 미리보기" /> : <ProviderLogo providerKind={session.provider_kind} size={40} />}
            {onAvatarUpdate && <Camera size={20} className="absolute" style={{ bottom: 0, right: 0 }} aria-hidden />}
          </button>
          <div className="min-w-0 flex-1">
            <strong className="preserve-words">{name || session.display_name}</strong>
            <p className="text-text-muted text-sm">방에서 보이는 이름과 사진이에요.</p>
          </div>
          {(photo || (!clearPhoto && session.avatar_image_url)) && <details className="relative">
            <summary className="dc-modal-close" aria-label="사진 옵션"><MoreHorizontal size={18} /></summary>
            <div className="dc-context-menu" style={{ position: "absolute", right: 0, left: "auto", top: "100%" }}>
              <button type="button" disabled={busy} onClick={(event) => {
                event.currentTarget.closest("details")?.removeAttribute("open");
                setPhoto(null); setClearPhoto(true);
              }}>사진 제거</button>
            </div>
          </details>}
        </div>
        <input ref={inputRef} type="file" accept="image/*" hidden aria-label="에이전트 프로필 사진 선택" disabled={busy || !onAvatarUpdate}
          onChange={(event) => { setCropFile(event.currentTarget.files?.[0] || null); event.currentTarget.value = ""; }} />
        <label className="dc-agent-field">표시 이름
          <input value={name} autoFocus disabled={busy} onChange={(event) => setName(event.currentTarget.value)} autoComplete="off" />
        </label>
        {!nameValid && <p className="dc-channel-composer-error preserve-words" role="alert">이름은 1~{AGENT_PROFILE_NAME_CHARACTER_LIMIT}자까지 입력할 수 있어요.</p>}
        {error && <p className="dc-channel-composer-error preserve-words" role="alert">{error}</p>}
        <footer className="dc-create-channel-actions" style={{ marginTop: 20 }}>
          <button type="button" className="dc-agent-create-secondary" style={{ minHeight: 44 }} disabled={busy} onClick={cancelEditing}>취소</button>
          <button type="submit" className="dc-agent-create-primary" style={{ minHeight: 44 }} disabled={busy || !nameValid || !changed}>{busy ? "저장 중…" : "변경사항 저장"}</button>
        </footer>
      </>}
    </form>
  );

  return <>
    {editing && canEdit ? editor : <>
    <header className="dc-member-detail-modal-head">
      <span className="dc-member-detail-modal-avatar">
        {avatarImage ? <img className="dc-member-avatar-image" src={avatarImage} alt="" /> : <ProviderLogo providerKind={session.provider_kind} size={48} />}
      </span>
      <div className="min-w-0 flex-1">
        <h2 className="truncate preserve-words">{session.display_name}</h2>
        {detail && <p className="truncate preserve-words">{detail}</p>}
      </div>
      {onSave && <button type="button" className="dc-modal-close" autoFocus aria-label="프로필 편집" title="프로필 편집" onClick={beginEditing}><Pencil size={18} /></button>}
      {onClose && <button type="button" className="dc-modal-close" aria-label="멤버 정보 닫기" onClick={onClose}><X size={18} /></button>}
    </header>
    {notice && <p className="dc-member-session-status preserve-words" role="status">{notice}</p>}
    </>}
    <div hidden={editing && canEdit}>{children}</div>
  </>;
}
