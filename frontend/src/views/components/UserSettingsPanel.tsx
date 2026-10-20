import { useResourceImage } from "./ResourceImage";
import { useEffect, useRef, type RefObject } from "react";
import { X, Camera, UserRound, Palette, KeyRound, MonitorSmartphone, type AppIcon } from "../../ui/icons";

import type { UserProfile, UserProfileIdentity } from "../../api";
import { resolveAttachmentReference } from "../../lib/attachmentReference";
import { profileCssVars } from "../../lib/userProfileModel";
import AppUpdateCheck from "./AppUpdateCheck";
import GoogleAccountSettings from "./GoogleAccountSettings";
import CentralAccountSettings from "./CentralAccountSettings";
import { centralIdentityConfigured } from "../../lib/central/identity";
import GuestRecoverySettings from "./GuestRecoverySettings";
import OwnerDevicesPanel from "./OwnerDevicesPanel";
import { isDesktopWebview } from "../../lib/desktopBridge";

export type UserSettingsSection = "account" | "profile" | "recovery" | "devices";

const USER_SETTINGS_SECTIONS: Array<{
  id: UserSettingsSection;
  label: string;
  icon: AppIcon;
}> = [
  { id: "account", label: "계정", icon: UserRound },
  { id: "profile", label: "프로필", icon: Palette },
  { id: "recovery", label: "복구", icon: KeyRound },
  { id: "devices", label: "기기", icon: MonitorSmartphone },
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
  serverAuthorityAvailable = true,
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
  serverAuthorityAvailable?: boolean;
}) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = dialogRef.current; dialog?.showModal();
    // Opening focuses the first control; keep that on the section being shown.
    dialog?.querySelector<HTMLElement>('[aria-current="page"]')?.focus();
    return () => { dialog?.close(); returnFocusRef.current?.focus(); };
  }, [returnFocusRef]);
  const canManageDevices = serverAuthorityAvailable && (Boolean(profileIdentity?.centralSession) || (!profileIdentity?.sessionToken && isDesktopWebview()));
  const sections = USER_SETTINGS_SECTIONS.filter(section => section.id === "recovery"
    ? profileIdentity?.sessionToken && !profileIdentity.centralOwner
    : section.id !== "devices" || canManageDevices);
  const draftAvatarUrl = useResourceImage(resolveAttachmentReference(
    draft.avatarImage,
    displayResourceBase
  )).url;
  // A section that stopped being available (authority or session changed while
  // open) falls back to 계정 instead of an empty page.
  const shownSection: UserSettingsSection = sections.some((section) => section.id === settingsSection) ? settingsSection : "account";
  const activeSectionLabel = sections.find((section) => section.id === shownSection)?.label || "계정";
  return (
    <dialog ref={dialogRef} className="dc-profile-settings-modal dc-user-settings-dialog" aria-label="사용자 설정"
      style={{ position: "fixed", inset: 0, margin: "auto", width: "min(1200px, calc(100vw - 80px))", height: "min(900px, calc(100dvh - 80px))", maxWidth: "calc(100vw - 32px)", maxHeight: "calc(100dvh - 32px)", padding: 0, color: "var(--color-text-primary)" }}
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
              disabled={saving}
              type="button"
              aria-current={shownSection === section.id ? "page" : undefined}
              onClick={() => onSectionChange(section.id)}
            >
              <section.icon size={20} aria-hidden />
              <span>{section.label}</span>
            </button>
          ))}
        </nav>
        <AppUpdateCheck />
      </aside>
      <div className="dc-user-settings-main">
        <header className="dc-user-settings-main-head">
          <h2>{activeSectionLabel}</h2>
          <button type="button" className="dc-dialog-close dc-user-settings-close" disabled={saving} aria-label="사용자 설정 닫기" onClick={onClose}><X size={24} /></button>
        </header>
        <section className="dc-user-settings-section">
          {shownSection === "account" && (
            <>
              {/* A non-owner never sees the local operator's profile as their
                  own: the signed-in central account is shown below (with its
                  own display name), so the local display name/handle/status
                  fields stay hidden here. */}
              {serverAuthorityAvailable && (
              <div className="dc-user-settings-grid">
                <h4>계정 정보</h4>
                <label>
                  표시 이름
                  <input
                    disabled={saving || !serverAuthorityAvailable}
                    value={draft.displayName}
                    onChange={(event) => onDraftChange({ ...draft, displayName: event.target.value })}
                    maxLength={120}
                  />
                </label>
                <label>
                  사용자명
                  <input
                    disabled={saving || !serverAuthorityAvailable}
                    value={draft.handle}
                    onChange={(event) => onDraftChange({ ...draft, handle: event.target.value })}
                    maxLength={120}
                  />
                </label>
                <label>
                  상태
                  <select
                    style={{ appearance: "none" }}
                    disabled={saving || !serverAuthorityAvailable}
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
              )}
              {centralIdentityConfigured() ? <>
                <CentralAccountSettings disabled={saving} />
                {profileIdentity?.sessionToken && !profileIdentity.centralOwner && <details className="mt-4">
                  <summary>이 서버의 Google 계정 연결</summary>
                  <GoogleAccountSettings identity={profileIdentity} />
                </details>}
              </> : <GoogleAccountSettings identity={profileIdentity ?? {}} />}
            </>
          )}

          {shownSection === "profile" && (
            <>
              {!serverAuthorityAvailable && (
                <p className="dc-user-settings-lead">내 서버가 있는 컴퓨터에서 프로필을 바꿀 수 있어요.</p>
              )}
              <h4 className="dc-user-settings-page-heading">프로필</h4>
              <div className="dc-user-settings-profile-layout">
              <div className="dc-user-settings-grid">
                <div className="dc-user-settings-avatar-field">
                  <span className="dc-user-settings-field-label">프로필 사진</span>
                  <div className="dc-user-settings-avatar-row">
                    <button
                      type="button"
                      className="dc-user-settings-avatar-tile"
                      disabled={saving || !serverAuthorityAvailable}
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
                    {/* The tile is the single way in, as in Discord; removal is the only extra action. */}
                    <div className="dc-user-settings-avatar-actions">
                      <span className="dc-user-settings-avatar-hint">사진을 눌러 바꿔요.</span>
                      {draft.avatarImage && (
                        <button
                          type="button"
                          className="dc-user-settings-text-button"
                          disabled={saving || !serverAuthorityAvailable}
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
                    disabled={saving || !serverAuthorityAvailable}
                    value={draft.customStatus}
                    onChange={(event) => onDraftChange({ ...draft, customStatus: event.target.value })}
                    maxLength={160}
                  />
                </label>
                <label>
                  배너
                  <select
                    style={{ appearance: "none" }}
                    disabled={saving || !serverAuthorityAvailable}
                    value={draft.bannerPreset}
                    onChange={(event) =>
                      onDraftChange({
                        ...draft,
                        bannerPreset: event.target.value as UserProfile["bannerPreset"],
                      })
                    }
                  >
                    <option value="default">기본 색상</option>
                    <option value="forest">그린</option>
                    <option value="midnight">미드나잇</option>
                    <option value="ember">엠버</option>
                    <option value="custom">포인트 색상</option>
                  </select>
                </label>
                <label>
                  사진 대신 보일 글자
                  <input
                    disabled={saving || !serverAuthorityAvailable}
                    value={draft.avatarLabel}
                    onChange={(event) => onDraftChange({ ...draft, avatarLabel: event.target.value })}
                    maxLength={2}
                  />
                </label>
                <label>
                  포인트 색상
                  <input
                    disabled={saving || !serverAuthorityAvailable}
                    type="color"
                    value={draft.accentColor}
                    onChange={(event) => onDraftChange({ ...draft, accentColor: event.target.value })}
                  />
                </label>
              </div>
              {/* Discord edits beside a live card; this one follows the unsaved draft. */}
              <aside className="dc-user-settings-preview" aria-label="프로필 미리보기">
                <span className="dc-user-settings-field-label">미리보기</span>
                <div className="dc-user-settings-preview-card" style={profileCssVars(draft, draftAvatarUrl)}>
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
                    {serverAuthorityAvailable && <small className="preserve-words">{draft.handle}</small>}
                    {draft.customStatus && <p className="preserve-words">{draft.customStatus}</p>}
                  </div>
                </div>
              </aside>
              </div>
            </>
          )}

          {shownSection === "recovery" && profileIdentity?.sessionToken && (
            <GuestRecoverySettings identity={profileIdentity} />
          )}
          {shownSection === "devices" && canManageDevices && <OwnerDevicesPanel identity={profileIdentity ?? {}} />}
        </section>
        {profileError && <p className="dc-user-settings-error" role="alert" style={{ position: "relative", inset: "auto", padding: "12px 32px", whiteSpace: "normal", overflow: "visible", overflowWrap: "anywhere" }}>{profileError}</p>}
        {/* Discord's save bar: it appears only once there is something to save. */}
        {serverAuthorityAvailable && shownSection !== "recovery" && (changed || saving) && (
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
