import { useEffect, useRef, type RefObject } from "react";
import { X, Camera, Headphones, Mic, MicOff } from "lucide-react";

import type { UserProfile, UserProfileIdentity } from "../../api";
import { resolveAttachmentReference } from "../../lib/attachmentReference";
import { profileCssVars } from "../../lib/userProfileModel";
import GoogleAccountSettings from "./GoogleAccountSettings";
import CentralAccountSettings from "./CentralAccountSettings";
import { centralIdentityConfigured } from "../../lib/centralIdentity";
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
  const sections = profileIdentity?.sessionToken && !profileIdentity.centralOwner
    ? USER_SETTINGS_SECTIONS
    : USER_SETTINGS_SECTIONS.filter((section) => section.id !== "recovery");
  const draftAvatarUrl = resolveAttachmentReference(
    draft.avatarImage,
    displayResourceBase
  );
  const activeSectionLabel =
    sections.find((section) => section.id === settingsSection)?.label || "사용자 설정";
  return (
    <dialog ref={dialogRef} className="dc-profile-settings-modal dc-user-settings-dialog" aria-label="사용자 설정"
      style={{ position: "fixed", inset: 0, margin: "auto", width: "min(980px, calc(100vw - 32px))", height: "min(760px, calc(100dvh - 32px))", maxHeight: "calc(100dvh - 32px)", padding: 0, color: "var(--color-text-primary)" }}
      onKeyDown={(event) => { if (event.key === "Escape") event.stopPropagation(); }}
      onCancel={(event) => { event.preventDefault(); if (!saving) onClose(); }}
      onClick={(event) => { if (event.target === event.currentTarget && !saving) onClose(); }}>
    <div className="dc-user-settings-layout">
      <aside className="dc-user-settings-sidebar">
        <div className="dc-user-settings-me">
          <span
            className="dc-user-settings-me-avatar"
            style={draftAvatarUrl ? { backgroundImage: `url(${draftAvatarUrl})` } : undefined}
            aria-hidden
          >
            {draftAvatarUrl ? null : draft.avatarLabel}
          </span>
          <span className="dc-user-settings-me-name preserve-words">{draft.displayName}</span>
        </div>
        <nav className="dc-user-settings-nav" aria-label="사용자 설정 섹션">
          {sections.map((section) => (
            <button
              key={section.id}
              style={{ minWidth: 44, minHeight: 44 }}
              disabled={saving}
              type="button"
              aria-current={settingsSection === section.id ? "page" : undefined}
              onClick={() => onSectionChange(section.id)}
            >
              <span>{section.label}</span>
            </button>
          ))}
        </nav>
      </aside>
      <div className="dc-user-settings-main">
        <header className="dc-user-settings-main-head">
          <h2>{activeSectionLabel}</h2>
          <button type="button" className="dc-profile-settings-close" style={{ minWidth: 44, minHeight: 44 }} disabled={saving} aria-label="사용자 설정 닫기" onClick={onClose}><X size={18} /></button>
        </header>
        <section className="dc-user-settings-section">
          {settingsSection === "account" && (
            <>
              <p className="dc-user-settings-lead">이 서버에서 사용할 이름과 표시 상태를 바꿔요.</p>
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
              {centralIdentityConfigured() ? <>
                <CentralAccountSettings disabled={saving} />
                {profileIdentity?.sessionToken && !profileIdentity.centralOwner && <details className="mt-4">
                  <summary>이 서버의 Google 계정 연결</summary>
                  <GoogleAccountSettings identity={profileIdentity} />
                </details>}
              </> : <GoogleAccountSettings identity={profileIdentity ?? {}} />}
            </>
          )}

          {settingsSection === "profile" && (
            <>
              <p className="dc-user-settings-lead">프로필 카드의 사진, 배너와 상태 문구를 바꿔요.</p>
              <div className="dc-user-settings-profile-layout">
              <div className="dc-user-settings-grid">
                <div className="dc-user-settings-avatar-field">
                  <span className="dc-user-settings-field-label">프로필 사진</span>
                  <div className="dc-user-settings-avatar-row">
                    <button
                      type="button"
                      className="dc-user-settings-avatar-tile"
                      disabled={saving}
                      onClick={onEditAvatar}
                      aria-label="프로필 사진 변경"
                      title="프로필 사진 변경"
                    >
                      <span
                        className="dc-user-settings-avatar-preview"
                        data-has-image={Boolean(draftAvatarUrl)}
                        style={draftAvatarUrl ? { backgroundImage: `url(${draftAvatarUrl})` } : undefined}
                        aria-hidden
                      >
                        {draftAvatarUrl ? null : draft.avatarLabel}
                      </span>
                      <span className="dc-user-settings-avatar-overlay" aria-hidden>
                        <Camera size={20} />
                      </span>
                    </button>
                    <div className="dc-user-settings-avatar-actions">
                      <button type="button" className="ops-cta min-h-11 px-4" disabled={saving} onClick={onEditAvatar}>
                        사진 변경
                      </button>
                      {draft.avatarImage && (
                        <button
                          type="button"
                          className="dc-user-settings-text-button"
                          disabled={saving}
                          onClick={() => onDraftChange({ ...draft, avatarImage: "" })}
                        >
                          사진 제거
                        </button>
                      )}
                    </div>
                  </div>
                </div>
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
                    <option value="default">기본</option>
                    <option value="forest">그린</option>
                    <option value="midnight">미드나잇</option>
                    <option value="ember">엠버</option>
                    <option value="custom">포인트 색상</option>
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
              {/* Discord edits beside a live card; this one follows the unsaved draft. */}
              <aside className="dc-user-settings-preview" aria-label="프로필 미리보기">
                <span className="dc-user-settings-field-label">미리보기</span>
                <div className="dc-user-settings-preview-card" style={profileCssVars(draft, displayResourceBase)}>
                  <div className="dc-profile-banner" data-preset={draft.bannerPreset} />
                  <span
                    className="dc-user-settings-preview-avatar"
                    style={draftAvatarUrl ? { backgroundImage: `url(${draftAvatarUrl})` } : undefined}
                    aria-hidden
                  >
                    {draftAvatarUrl ? null : draft.avatarLabel}
                  </span>
                  <div className="dc-user-settings-preview-body">
                    <strong className="preserve-words">{draft.displayName || "이름 없음"}</strong>
                    <small className="preserve-words">{draft.handle}</small>
                    {draft.customStatus && <p className="preserve-words">{draft.customStatus}</p>}
                  </div>
                </div>
              </aside>
              </div>
            </>
          )}

          {settingsSection === "voice" && (
            <>
              <p className="dc-user-settings-lead">실제 음성 연결은 아니고, 방 클라이언트의 표시 상태만 저장합니다.</p>
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
        {profileError && <p className="dc-user-settings-error" role="alert" style={{ position: "relative", inset: "auto", padding: "12px 32px", whiteSpace: "normal", overflow: "visible", overflowWrap: "anywhere" }}>{profileError}</p>}
        {/* Discord's save bar: it appears only once there is something to save. */}
        {settingsSection !== "recovery" && (changed || saving) && (
          <div className="dc-user-settings-savebar" role="region" aria-label="저장하지 않은 변경 사항">
            <span className="dc-user-settings-savebar-text">저장하지 않은 변경 사항이 있어요.</span>
            <button type="button" onClick={onReset} style={{ minWidth: 44, minHeight: 44 }} disabled={saving || !changed}>
              되돌리기
            </button>
            <button type="button" onClick={onSave} style={{ minWidth: 44, minHeight: 44 }} disabled={saving || !changed || !draft.displayName.trim()}>
              {saving ? "저장 중" : "저장"}
            </button>
          </div>
        )}
      </div>
    </div>
    </dialog>
  );
}
