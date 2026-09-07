import { useEffect, useRef, useState } from "react";
import ImageCropper from "../ImageCropper";
import type { RoomAgentSession } from "../../../api";

export default function AgentIdentitySettings({ session, onSave, onAvatarUpdate }: {
  session: RoomAgentSession;
  onAvatarUpdate?: (session: RoomAgentSession, file: File, displayName: string, signal: AbortSignal) => Promise<void>;
  onSave: (session: RoomAgentSession, settings: Record<string, string>) => void | Promise<void>;
}) {
  const avatarInputRef = useRef<HTMLInputElement>(null);
  const uploadRef = useRef<AbortController | null>(null);
  const [cropFile, setCropFile] = useState<File | null>(null);
  useEffect(() => () => uploadRef.current?.abort(), []);
  const [name, setName] = useState(session.display_name);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  useEffect(() => { setName(session.display_name); }, [session.display_name]);

  async function saveProfile(clearAvatar = false) {
    if (busy) return;
    setBusy(true);
    setStatus("");
    try {
      await onSave(session, { display_name: name, ...(clearAvatar ? { avatar_image_url: "" } : {}) });
      setStatus("에이전트 프로필 저장됨");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "에이전트 프로필 저장 실패");
    } finally {
      setBusy(false);
    }
  }

  async function saveAvatar(file: File) {
    if (!onAvatarUpdate || busy || uploadRef.current) return;
    const controller = new AbortController();
    uploadRef.current = controller;
    setBusy(true);
    setCropFile(null);
    setStatus("프로필 사진 저장 중...");
    try {
      await onAvatarUpdate(session, file, name, controller.signal);
      if (!controller.signal.aborted) setStatus("프로필 사진 저장됨");
    } catch (error) {
      if (!controller.signal.aborted) setStatus(error instanceof Error ? error.message : "프로필 사진 저장 실패");
    } finally {
      if (!controller.signal.aborted) setBusy(false);
      if (uploadRef.current === controller) uploadRef.current = null;
    }
  }

  return (
    <section className="dc-agent-runtime-settings" aria-label={`${session.display_name} 에이전트 프로필`}>
      <label>
        <span>표시 이름</span>
        <input type="text" value={name} disabled={busy}
          onChange={(event) => setName(event.currentTarget.value)}
          placeholder={session.display_name} />
      </label>
      <button type="button" className="dc-member-session-button" disabled={busy || !name.trim()}
        onClick={() => void saveProfile()}>
        프로필 저장
      </button>
      {onAvatarUpdate && <>
        <input ref={avatarInputRef} className="sr-only" type="file" accept="image/*"
          aria-label="에이전트 프로필 사진 선택" disabled={busy}
          onChange={(event) => { setCropFile(event.currentTarget.files?.[0] || null); event.currentTarget.value = ""; }} />
        <button type="button" className="dc-member-session-button" disabled={busy}
          onClick={() => avatarInputRef.current?.click()}>프로필 사진 변경</button>
      </>}
      {session.avatar_image_url && <button type="button" className="dc-member-session-button" disabled={busy}
        onClick={() => void saveProfile(true)}>프로필 사진 삭제</button>}
      {cropFile && <ImageCropper file={cropFile} onCancel={() => setCropFile(null)} onCropped={(file) => void saveAvatar(file)} />}
      {status && <p className="dc-member-session-status preserve-words" role="status">{status}</p>}
    </section>
  );
}
