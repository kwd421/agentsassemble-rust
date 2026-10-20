import ResourceImage from "../ResourceImage";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Camera, X } from "../../../ui/icons";
import { AGENT_PROFILE_NAME_CHARACTER_LIMIT } from "../../../types/generated/AGENT_PROFILE_WIRE";
import type { RoomAgentSession } from "../../../api";
import ImageCropDialog from "../ImageCropDialog";
import ProviderLogo from "../ProviderLogo";
import { userFacingError } from "../../../lib/apiErrors";

// Edited in place like the account profile: the photo opens the file picker and then
// the crop dialog, and the name is its own edit box. No separate edit screen.
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
  const inFlightRef = useRef(false);
  const [name, setName] = useState(session.display_name);
  const [nameFocused, setNameFocused] = useState(false);
  const nameFocusedRef = useRef(false);
  const discardNameRef = useRef(false);
  const [cropFile, setCropFile] = useState<File | null>(null);
  const [photoMenu, setPhotoMenu] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [cropStatus, setCropStatus] = useState("");
  useEffect(() => () => uploadRef.current?.abort(), []);
  // The box follows a newly stored name unless the person is typing in it; a failed
  // save keeps their draft.
  useEffect(() => { if (!nameFocusedRef.current) setName(session.display_name); }, [session.display_name]);
  const canEditName = Boolean(onSave);
  const canEditPhoto = Boolean(onAvatarUpdate);
  const hasPhoto = Boolean(session.avatar_image_url && avatarImage);
  const trimmed = name.trim();
  const nameValid = trimmed.length > 0 && Array.from(trimmed).length <= AGENT_PROFILE_NAME_CHARACTER_LIMIT;

  async function run(action: (signal: AbortSignal) => Promise<void> | void, failure: string): Promise<boolean> {
    if (inFlightRef.current) return false;
    inFlightRef.current = true;
    setBusy(true);
    setError("");
    const controller = new AbortController();
    uploadRef.current = controller;
    try {
      await action(controller.signal);
      return !controller.signal.aborted;
    } catch (reason) {
      if (!controller.signal.aborted) setError(userFacingError(reason, failure));
      return false;
    } finally {
      inFlightRef.current = false;
      if (!controller.signal.aborted) setBusy(false);
      if (uploadRef.current === controller) uploadRef.current = null;
    }
  }

  async function saveName() {
    if (!onSave || !nameValid || trimmed === session.display_name) {
      if (!nameValid) setName(session.display_name);
      return;
    }
    await run(() => onSave(session, { display_name: trimmed }), "이름을 저장하지 못했어요. 다시 시도해 주세요.");
  }

  async function applyPhoto(file: File) {
    if (!onAvatarUpdate) return;
    setCropStatus("");
    const saved = await run((signal) => onAvatarUpdate(session, file, session.display_name, signal),
      "사진을 저장하지 못했어요. 다시 시도해 주세요.");
    if (saved) setCropFile(null);
    else setCropStatus("사진을 저장하지 못했어요. 다시 시도해 주세요.");
  }

  function pickPhoto() {
    setPhotoMenu(false);
    inputRef.current?.click();
  }

  return <>
    <header className="dc-member-detail-modal-head">
      <span className="relative">
        {canEditPhoto ? <button type="button" className="dc-member-detail-modal-avatar dc-agent-avatar-edit"
          aria-label="프로필 사진 변경" title="프로필 사진 변경" disabled={busy}
          aria-haspopup={hasPhoto && canEditName ? "menu" : undefined} aria-expanded={hasPhoto && canEditName ? photoMenu : undefined}
          onClick={() => (hasPhoto && canEditName ? setPhotoMenu((open) => !open) : pickPhoto())}>
          {avatarImage ? <ResourceImage className="dc-member-avatar-image" src={avatarImage} alt="" /> : <ProviderLogo providerKind={session.provider_kind} size={48} />}
          <Camera size={16} className="dc-agent-avatar-edit-badge" aria-hidden />
        </button> : <span className="dc-member-detail-modal-avatar">
          {avatarImage ? <ResourceImage className="dc-member-avatar-image" src={avatarImage} alt="" /> : <ProviderLogo providerKind={session.provider_kind} size={48} />}
        </span>}
        {photoMenu && <div className="dc-context-menu" role="menu" style={{ position: "absolute", left: 0, top: "100%", zIndex: 5 }}
          onKeyDown={(event) => { if (event.key === "Escape") { event.stopPropagation(); setPhotoMenu(false); } }}>
          <button type="button" role="menuitem" autoFocus onClick={pickPhoto}>사진 바꾸기</button>
          <button type="button" role="menuitem" onClick={() => {
            setPhotoMenu(false);
            void run(() => onSave?.(session, { display_name: session.display_name, avatar_image_url: "" }),
              "사진을 지우지 못했어요. 다시 시도해 주세요.");
          }}>사진 제거</button>
        </div>}
      </span>
      <div className="min-w-0 flex-1">
        {canEditName ? <input className="dc-agent-name-input preserve-words" aria-label="표시 이름" value={name}
          disabled={busy} autoComplete="off" spellCheck={false}
          onFocus={() => { nameFocusedRef.current = true; setNameFocused(true); }}
          onChange={(event) => setName(event.currentTarget.value)}
          onBlur={() => {
            nameFocusedRef.current = false;
            setNameFocused(false);
            if (discardNameRef.current) { discardNameRef.current = false; return; }
            void saveName();
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter") { event.preventDefault(); event.currentTarget.blur(); }
            if (event.key === "Escape") {
              event.preventDefault();
              event.stopPropagation();
              discardNameRef.current = true;
              setName(session.display_name);
              setError("");
              event.currentTarget.blur();
            }
          }} /> : <h2 className="truncate preserve-words">{session.display_name}</h2>}
        {detail && <p className="truncate preserve-words">{detail}</p>}
      </div>
      {onClose && <button type="button" className="dc-modal-close" aria-label="멤버 정보 닫기" onClick={onClose}><X size={24} /></button>}
    </header>
    {nameFocused && !nameValid && <p className="dc-channel-composer-error preserve-words" role="alert">이름은 1~{AGENT_PROFILE_NAME_CHARACTER_LIMIT}자까지 입력할 수 있어요.</p>}
    {error && <p className="dc-channel-composer-error preserve-words" role="alert">{error}</p>}
    {canEditPhoto && <input ref={inputRef} type="file" accept="image/*" hidden aria-label="에이전트 프로필 사진 선택" disabled={busy}
      onChange={(event) => { const file = event.currentTarget.files?.[0] || null; event.currentTarget.value = ""; if (file) { setCropStatus(""); setCropFile(file); } }} />}
    {cropFile && <ImageCropDialog title="프로필 사진 수정" file={cropFile} shape="circle" busy={busy} status={cropStatus}
      onCancel={() => { uploadRef.current?.abort(); setCropFile(null); setCropStatus(""); }}
      onApply={(file) => void applyPhoto(file)} />}
    {children}
  </>;
}
