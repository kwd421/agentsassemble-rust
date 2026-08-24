import { useEffect, useState, type RefObject } from "react";
import { uploadLobbyAttachment, type RoomAgentSession } from "../../../api";
import ImageCropper from "../ImageCropper";
import type { MemberEntry } from "./memberTypes";
import "./AgentIdentitySettings.css";

export default function AgentIdentitySettings({
  entry,
  agent,
  avatarInputRef,
  roomSessionToken = "",
  onSessionActionComplete,
  onAgentConfigure,
}: {
  entry: MemberEntry;
  agent: NonNullable<MemberEntry["agent"]>;
  avatarInputRef?: RefObject<HTMLInputElement | null>;
  roomSessionToken?: string;
  onSessionActionComplete?: () => void;
  onAgentConfigure?: (
    session: RoomAgentSession,
    settings: Record<string, string>
  ) => void | Promise<void>;
}) {
  const [name, setName] = useState(
    entry.agentDisplayName || entry.displayName || ""
  );
  const [avatar, setAvatar] = useState(entry.avatarImage || "");
  const [cropFile, setCropFile] = useState<File | null>(null);
  const [status, setStatus] = useState("");

  useEffect(() => {
    setName(
      entry.agentDisplayName || entry.displayName || ""
    );
    setAvatar(entry.avatarImage || "");
    setStatus("");
  }, [
    entry.agent?.agent_id,
    entry.agentDisplayName,
    entry.avatarImage,
    entry.displayName,
  ]);

  async function persistProfile(nextAvatar: string) {
    if (!entry.agentSession || !onAgentConfigure) {
      throw new Error("이 에이전트 프로필의 Agent Session 권위를 사용할 수 없습니다.");
    }
    await onAgentConfigure(entry.agentSession, {
      display_name: name,
      avatar_image_url: nextAvatar,
    });
    onSessionActionComplete?.();
  }

  async function saveAvatar(file: File) {
    setStatus("프로필 사진 저장 중...");
    try {
      const attachment = await uploadLobbyAttachment(file, {
        purpose: "profile_avatar",
        sessionToken: roomSessionToken,
      });
      setAvatar(attachment.url);
      await persistProfile(attachment.url);
      setCropFile(null);
      setStatus("프로필 사진 저장됨");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "프로필 사진 저장 실패");
    }
  }

  async function saveProfile() {
    setStatus("에이전트 프로필 저장 중...");
    try {
      await persistProfile(avatar);
      setStatus("에이전트 프로필 저장됨");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "에이전트 프로필 저장 실패");
    }
  }

  if (!entry.ownedByViewer || !entry.agentSession || !onAgentConfigure) return null;
  return (
    <section className="dc-agent-profile-inline" aria-label={`${entry.displayName} 에이전트 프로필`}>
      <label className="dc-agent-profile-field">
        <span>표시 이름</span>
        <input
          type="text"
          maxLength={80}
          value={name}
          onChange={(event) => setName(event.currentTarget.value)}
          placeholder={agent.display_name || agent.agent_id}
        />
      </label>
      <input
        ref={avatarInputRef}
        className="sr-only"
        type="file"
        aria-label="에이전트 프로필 사진 선택"
        accept="image/*"
        onChange={(event) => {
          const file = event.currentTarget.files?.[0] || null;
          if (file) setCropFile(file);
          event.currentTarget.value = "";
        }}
      />
      <div className="dc-agent-profile-inline-actions">
        <button type="button" className="dc-member-session-button" onClick={() => void saveProfile()}>
          프로필 저장
        </button>
      </div>
      {cropFile && (
        <ImageCropper
          file={cropFile}
          onCancel={() => setCropFile(null)}
          onCropped={(file) => void saveAvatar(file)}
        />
      )}
      {status && <p className="dc-member-session-status preserve-words">{status}</p>}
    </section>
  );
}
