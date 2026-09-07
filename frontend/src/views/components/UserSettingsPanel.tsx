import { useEffect, useRef, type RefObject } from "react";
import { X, Camera, Headphones, Mic, MicOff, Palette, UserCircle } from "lucide-react";

import type { UserProfile, UserProfileIdentity } from "../../api";
import { resolveAttachmentReference } from "../../lib/attachmentReference";
import GoogleAccountSettings from "./GoogleAccountSettings";
import GuestRecoverySettings from "./GuestRecoverySettings";

export type UserSettingsSection = "account" | "profile" | "voice" | "recovery";

const USER_SETTINGS_SECTIONS: Array<{
  id: UserSettingsSection;
  label: string;
}> = [
  { id: "account", label: "계정" },
  { id: "profile", label: "프로필" },
  { id: "voice", label: "음성" },
  { id: "recovery", label: "복구" },
];

export default function UserSettingsPanel({
  draft,
  saving, onClose, changed, returnFocusRef,
  profileError,
  settingsSection,
  onSectionChange,
  onDraftChange,
  onReset,
  onSave,
  onEditAvatar,
  profileIdentity,
  displayResourceBase,
}: {
  onClose: () => void;
  returnFocusRef: RefObject<HTMLButtonElement | null>;
  changed: boolean;
  draft: UserProfile;
  saving: boolean;
  profileError: string;
  settingsSection: UserSettingsSection;
  onSectionChange: (section: UserSettingsSection) => void;
  onDraftChange: (profile: UserProfile) => void;
  onReset: () => void;
  onSave: () => void;
  onEditAvatar: () => void;
  profileIdentity?: UserProfileIdentity;
  displayResourceBase: string;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = dialogRef.current; dialog?.showModal();
    return () => { dialog?.close(); returnFocusRef.current?.focus(); };
  }, [returnFocusRef]);
  const sections = profileIdentity?.sessionToken
    ? USER_SETTINGS_SECTIONS
    : USER_SETTINGS_SECTIONS.filter((section) => section.id !== "recovery");
  const draftAvatarUrl = resolveAttachmentReference(
    draft.avatarImage,
    displayResourceBase
  );
  return (
    <dialog ref={dialogRef} className="dc-profile-settings-modal" aria-label="사용자 설정"
      style={{ position: "fixed", inset: 0, margin: "auto", width: "min(620px, calc(100vw - 32px))", height: "min(720px, calc(100dvh - 32px))", maxHeight: "calc(100dvh - 32px)", color: "var(--color-text-primary)" }}
      onKeyDown={(event) => { if (event.key === "Escape") event.stopPropagation(); }}
      onCancel={(event) => { event.preventDefault(); if (!saving) onClose(); }}
      onClick={(event) => { if (event.target === event.currentTarget && !saving) onClose(); }}>
      <header className="dc-profile-settings-header" style={{ padding: "16px 24px" }}>
        <h2>사용자 설정</h2>
        <button type="button" className="dc-profile-settings-close" style={{ minWidth: 44, minHeight: 44 }} disabled={saving} aria-label="사용자 설정 닫기" onClick={onClose}><X size={18} /></button>
      </header>
    <div className="dc-user-settings-panel" aria-label="사용자 설정" style={{ gridTemplateRows: "minmax(0, 1fr) auto auto" }}>
      <div className="dc-user-settings-shell" style={{ gridTemplateColumns: "minmax(0, 1fr)", gridTemplateRows: "auto minmax(0, 1fr)", gap: 20, padding: "20px 24px 0" }}>
        <nav className="dc-user-settings-nav" aria-label="사용자 설정 섹션" style={{ display: "flex", flexWrap: "wrap", gap: 8, paddingBottom: 0 }}>
          {sections.map((section) => (
            <button
              key={section.id}
              style={{ minWidth: 44, minHeight: 44, padding: "8px 12px" }}
              disabled={saving}
              type="button"
              aria-current={settingsSection === section.id ? "page" : undefined}
              onClick={() => onSectionChange(section.id)}
            >
              <span>{section.label}</span>

            </button>
          ))}
        </nav>

        <section className="dc-user-settings-section" style={{ paddingRight: 4, paddingBottom: 24 }}>
          {settingsSection === "account" && (
            <>
              <header>
                <UserCircle size={18} />
                <div>
                  <h3>계정</h3>
                  <p>이 서버에서 사용할 이름과 표시 상태를 바꿔요.</p>
                </div>
              </header>
              <div className="dc-user-settings-grid">
                <label>
                  표시 이름
                  <input
                    style={{ minHeight: 44 }}
                    disabled={saving}
                    value={draft.displayName}
                    onChange={(event) => onDraftChange({ ...draft, displayName: event.target.value })}
                    maxLength={120}
                  />
                </label>
                <label>
                  핸들
                  <input
                    style={{ minHeight: 44 }}
                    disabled={saving}
                    value={draft.handle}
                    onChange={(event) => onDraftChange({ ...draft, handle: event.target.value })}
                    maxLength={120}
                  />
                </label>
                <label>
                  상태
                  <select
                    style={{ minHeight: 44, appearance: "none" }}
                    disabled={saving}
                    value={draft.status}
                    onChange={(event) =>
                      onDraftChange({
                        ...draft,
                        status: event.target.value as UserProfile["status"],
                      })
                    }
                  >
                    <option value="online">온라인</option>
                    <option value="idle">자리 비움</option>
                    <option value="dnd">방해 금지</option>
                    <option value="offline">오프라인 표시</option>
                  </select>
                </label>
              </div>
              <GoogleAccountSettings identity={profileIdentity ?? {}} />
            </>
          )}

          {settingsSection === "profile" && (
            <>
              <header>
                <Palette size={18} />
                <div>
                  <h3>프로필</h3>
                  <p>프로필 카드의 배너와 상태 문구를 바꿔요.</p>
                </div>
              </header>
              <div className="dc-user-settings-grid">
                <button
                  type="button"
                  className="dc-user-settings-avatar-action"
                  style={{ minHeight: 44 }} disabled={saving}
                  onClick={onEditAvatar}
                  aria-label="프로필 사진 변경"
                >
                  <span
                    className="dc-user-settings-avatar-preview"
                    data-has-image={Boolean(draftAvatarUrl)}
                    style={
                      draftAvatarUrl
                        ? {
                            backgroundImage: `url(${draftAvatarUrl})`,
                          }
                        : undefined
                    }
                    aria-hidden
                  >
                    {draftAvatarUrl ? null : draft.avatarLabel}
                  </span>
                  <span>
                    <strong>프로필 사진 변경</strong>
                    <small>사진을 고르고 표시할 영역을 조정해요.</small>
                  </span>
                  <Camera size={17} aria-hidden />
                </button>
                <label>
                  사용자 지정 상태
                  <input
                    style={{ minHeight: 44 }}
                    disabled={saving}
                    value={draft.customStatus}
                    onChange={(event) => onDraftChange({ ...draft, customStatus: event.target.value })}
                    maxLength={160}
                  />
                </label>
                <label>
                  배너
                  <select
                    style={{ minHeight: 44, appearance: "none" }}
                    disabled={saving}
                    value={draft.bannerPreset}
                    onChange={(event) =>
                      onDraftChange({
                        ...draft,
                        bannerPreset: event.target.value as UserProfile["bannerPreset"],
                      })
                    }
                  >
                    <option value="default">Discord blue</option>
                    <option value="forest">Forest</option>
                    <option value="midnight">Midnight</option>
                    <option value="ember">Ember</option>
                    <option value="custom">사용자 색상</option>
                  </select>
                </label>
                <label>
                  아바타 라벨
                  <input
                    style={{ minHeight: 44 }}
                    disabled={saving}
                    value={draft.avatarLabel}
                    onChange={(event) => onDraftChange({ ...draft, avatarLabel: event.target.value })}
                    maxLength={2}
                  />
                </label>
                <label>
                  포인트 색상
                  <input
                    style={{ minHeight: 44 }}
                    disabled={saving}
                    type="color"
                    value={draft.accentColor}
                    onChange={(event) => onDraftChange({ ...draft, accentColor: event.target.value })}
                  />
                </label>
              </div>
            </>
          )}

          {settingsSection === "voice" && (
            <>
              <header>
                <Headphones size={18} />
                <div>
                  <h3>음성</h3>
                  <p>실제 음성 연결은 아니고, 방 클라이언트의 표시 상태만 저장합니다.</p>
                </div>
              </header>
              <div className="dc-user-settings-toggles">
                <button
                  type="button"
                  style={{ minHeight: 44 }} disabled={saving}
                  aria-pressed={draft.micMuted}
                  onClick={() => onDraftChange({ ...draft, micMuted: !draft.micMuted })}
                >
                  {draft.micMuted ? <MicOff size={18} /> : <Mic size={18} />}
                  <span>{draft.micMuted ? "마이크 음소거 표시" : "마이크 켜짐 표시"}</span>
                </button>
                <button
                  type="button"
                  style={{ minHeight: 44 }} disabled={saving}
                  aria-pressed={draft.deafened}
                  onClick={() => onDraftChange({ ...draft, deafened: !draft.deafened })}
                >
                  <Headphones size={18} />
                  <span>{draft.deafened ? "헤드셋 차단 표시" : "헤드셋 사용 표시"}</span>
                </button>
              </div>
            </>
          )}

          {settingsSection === "recovery" && profileIdentity?.sessionToken && (
            <GuestRecoverySettings identity={profileIdentity} />
          )}
        </section>
      </div>
      <div>{profileError && <p className="dc-user-settings-error" role="alert" style={{ position: "relative", inset: "auto", padding: "12px 24px", whiteSpace: "normal", overflow: "visible", overflowWrap: "anywhere" }}>{profileError}</p>}</div>
      {settingsSection !== "recovery" && (
        <div className="dc-user-settings-actions" style={{ padding: "16px 24px", gap: 12 }}>
          <button type="button" onClick={onReset} style={{ minWidth: 44, minHeight: 44 }} disabled={saving || !changed}>
            되돌리기
          </button>
          <button type="button" onClick={onSave} style={{ minWidth: 44, minHeight: 44 }} disabled={saving || !changed || !draft.displayName.trim()}>
            {saving ? "저장 중" : "저장"}
          </button>
        </div>
      )}
    </div>
    </dialog>
  );
}
