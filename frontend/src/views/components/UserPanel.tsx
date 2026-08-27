import { useEffect, useRef, useState } from "react";
import {
  ChevronDown,
  Headphones,
  LogOut,
  Mic,
  MicOff,
  Pencil,
  Settings,
  UserPen,
  X,
} from "lucide-react";

import {
  fetchUserProfile,
  saveUserProfile,
  uploadUserProfileAvatar,
  type UserProfile,
  type UserProfileIdentity,
  type UserProfileSnapshot,
} from "../../api";
import {
  DEFAULT_USER_PROFILE,
  PROFILE_STATUS_OPTIONS,
  profileCssVars,
  profileStatusClass,
  profileStatusLabel,
} from "../../lib/userProfileModel";
import ImageCropper from "./ImageCropper";
import UserSettingsPanel, { type UserSettingsSection } from "./UserSettingsPanel";

export default function UserPanel({
  onlineCount,
  agentCount,
  hasBackendError,
  guestProfile,
  profileIdentity = {},
  onGuestExit,
}: {
  onlineCount: number;
  agentCount: number;
  hasBackendError: boolean;
  guestProfile?: {
    displayName: string;
    avatarLabel: string;
    avatarImage?: string;
    statusLabel: string;
    expired?: boolean;
  };
  profileIdentity?: UserProfileIdentity;
  onGuestExit?: () => void;
}) {
  const initialGuestName = String(guestProfile?.displayName || "").trim();
  const initialProfile = guestProfile
    ? {
        ...DEFAULT_USER_PROFILE,
        displayName: initialGuestName || "게스트",
        avatarLabel: String(
          guestProfile.avatarLabel || initialGuestName.slice(0, 2) || "G"
        )
          .trim()
          .slice(0, 2)
          .toUpperCase(),
        avatarImage: guestProfile.avatarImage,
      }
    : DEFAULT_USER_PROFILE;
  const [profileSnapshot, setProfileSnapshot] = useState({
    profile: initialProfile,
    displayResourceBase: "",
  });
  const profile = profileSnapshot.profile;
  const [draft, setDraft] = useState<UserProfile>(initialProfile);
  const [profileOpen, setProfileOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<UserSettingsSection>("account");
  const [statusMenuOpen, setStatusMenuOpen] = useState(false);
  const [avatarCropFile, setAvatarCropFile] = useState<File | null>(null);
  const [avatarEditorOpen, setAvatarEditorOpen] = useState(false);
  const [avatarStatus, setAvatarStatus] = useState("");
  const [saving, setSaving] = useState(false);
  const [profileError, setProfileError] = useState("");
  const [profileHydrated, setProfileHydrated] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const profileSnapshotRef = useRef(profileSnapshot);
  const profileScopeGeneration = useRef(0);
  const profileIntentGeneration = useRef(0);
  const profileWriteGeneration = useRef(0);
  const profileWriteTail = useRef<Promise<void>>(Promise.resolve());
  const avatarSubmissionInFlight = useRef(false);
  const statusClass = profileStatusClass(profile, hasBackendError);
  const hasAvatarImage = Boolean(profile.avatarImage);
  const guestDisplayName = String(guestProfile?.displayName || "게스트").trim() || "게스트";
  const guestAvatarLabel = String(guestProfile?.avatarLabel || guestDisplayName.slice(0, 1) || "G")
    .trim()
    .slice(0, 2)
    .toUpperCase();
  const guestHasAvatarImage = Boolean(guestProfile?.avatarImage);
  const guestAwaitingAdmission = Boolean(guestProfile && !profileIdentity.sessionToken);

  useEffect(() => {
    const generation = ++profileScopeGeneration.current;
    profileIntentGeneration.current += 1;
    profileWriteGeneration.current += 1;
    setSaving(false);
    if (guestProfile?.expired || guestAwaitingAdmission) {
      setProfileHydrated(false);
      return;
    }
    setProfileHydrated(false);
    setProfileError("");
    const hydration = profileWriteTail.current.then(async () => {
      if (profileScopeGeneration.current !== generation) return;
      try {
        const loadedSnapshot = await fetchUserProfile(profileIdentity);
        if (profileScopeGeneration.current !== generation) return;
        profileSnapshotRef.current = loadedSnapshot;
        setProfileSnapshot(loadedSnapshot);
        setDraft(loadedSnapshot.profile);
        setProfileHydrated(true);
      } catch (error) {
        if (profileScopeGeneration.current !== generation) return;
        setProfileHydrated(false);
        setProfileError(
          error instanceof Error && error.message
            ? error.message
            : "프로필을 불러오지 못했습니다."
        );
      }
    });
    profileWriteTail.current = hydration;
    return () => {
      if (profileScopeGeneration.current === generation) {
        profileScopeGeneration.current += 1;
        profileIntentGeneration.current += 1;
        profileWriteGeneration.current += 1;
      }
    };
  }, [
    guestProfile?.expired,
    guestAwaitingAdmission,
    profileIdentity.deviceToken,
    profileIdentity.sessionToken,
  ]);

  useEffect(() => {
    if (!profileOpen && !settingsOpen && !avatarEditorOpen) return;
    function closeOnOutside(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) {
        setProfileOpen(false);
        setSettingsOpen(false);
        setStatusMenuOpen(false);
        setAvatarEditorOpen(false);
      }
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setProfileOpen(false);
        setSettingsOpen(false);
        setStatusMenuOpen(false);
        setAvatarEditorOpen(false);
      }
    }
    window.addEventListener("mousedown", closeOnOutside);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("mousedown", closeOnOutside);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [avatarEditorOpen, profileOpen, settingsOpen]);

  function openProfile() {
    setDraft(profile);
    setProfileOpen((value) => !value);
    setSettingsOpen(false);
    setStatusMenuOpen(false);
  }

  function openSettings(section: UserSettingsSection = "account") {
    setDraft(profile);
    setProfileOpen(false);
    setSettingsOpen(true);
    setSettingsSection(section);
  }

  function openAvatarEditor() {
    setProfileOpen(false);
    setSettingsOpen(false);
    setAvatarCropFile(null);
    setAvatarStatus("");
    setAvatarEditorOpen(true);
  }

  async function enqueueProfileOperation(
    execute: (currentProfile: UserProfile) => Promise<UserProfileSnapshot>
  ): Promise<"saved" | "stale" | "failed"> {
    const scopeGeneration = profileScopeGeneration.current;
    const intentGeneration = profileIntentGeneration.current;
    const writeGeneration = ++profileWriteGeneration.current;
    setSaving(true);
    const operation = async (): Promise<"saved" | "stale" | "failed"> => {
      try {
        if (
          profileScopeGeneration.current !== scopeGeneration ||
          profileIntentGeneration.current !== intentGeneration
        ) {
          return "stale";
        }
        setProfileError("");
        try {
          const savedSnapshot = await execute(profileSnapshotRef.current.profile);
          if (profileScopeGeneration.current !== scopeGeneration) return "stale";
          profileSnapshotRef.current = savedSnapshot;
          setProfileSnapshot(savedSnapshot);
          setDraft(savedSnapshot.profile);
          return "saved";
        } catch (error) {
          if (profileScopeGeneration.current !== scopeGeneration) return "stale";
          const message =
            error instanceof Error ? error.message : "프로필을 저장하지 못했습니다.";
          const recoveryIntentGeneration = ++profileIntentGeneration.current;
          setProfileHydrated(false);
          setProfileError(message);
          try {
            const recoveredSnapshot = await fetchUserProfile(profileIdentity);
            if (profileScopeGeneration.current !== scopeGeneration) return "stale";
            profileSnapshotRef.current = recoveredSnapshot;
            setProfileSnapshot(recoveredSnapshot);
            setDraft(recoveredSnapshot.profile);
            setProfileHydrated(true);
          } catch (recoveryError) {
            if (profileScopeGeneration.current !== scopeGeneration) return "stale";
            const recoveryMessage =
              recoveryError instanceof Error && recoveryError.message
                ? recoveryError.message
                : "서버 프로필을 다시 확인하지 못했습니다.";
            setProfileError(`${message} ${recoveryMessage}`);
          } finally {
            if (
              profileScopeGeneration.current === scopeGeneration &&
              profileIntentGeneration.current === recoveryIntentGeneration
            ) {
              profileIntentGeneration.current += 1;
            }
          }
          return "failed";
        }
      } finally {
        if (
          profileScopeGeneration.current === scopeGeneration &&
          profileWriteGeneration.current === writeGeneration
        ) {
          setSaving(false);
        }
      }
    };
    const result = profileWriteTail.current.then(operation);
    profileWriteTail.current = result.then(() => undefined);
    return result;
  }

  function persistProfile(
    applyMutation: (currentProfile: UserProfile) => UserProfile
  ): Promise<"saved" | "stale" | "failed"> {
    return enqueueProfileOperation((currentProfile) =>
      saveUserProfile(applyMutation(currentProfile), profileIdentity)
    );
  }

  function updateProfileFlag(key: "micMuted" | "deafened", value: boolean) {
    void persistProfile((currentProfile) => ({ ...currentProfile, [key]: value }));
  }

  function setProfileStatus(status: UserProfile["status"]) {
    void persistProfile((currentProfile) => ({ ...currentProfile, status }));
    setStatusMenuOpen(false);
  }

  async function saveDraft() {
    if ((await persistProfile(() => draft)) === "saved") setSettingsOpen(false);
  }

  async function handleAvatarCropped(file: File) {
    if (avatarSubmissionInFlight.current) return;
    avatarSubmissionInFlight.current = true;
    setAvatarStatus("프로필 사진 저장 중...");
    try {
      const result = await enqueueProfileOperation(async (currentProfile) => {
        const avatarImage = await uploadUserProfileAvatar(file, profileIdentity);
        return saveUserProfile({ ...currentProfile, avatarImage }, profileIdentity);
      });
      if (result === "stale") return;
      if (result === "failed") {
        setAvatarStatus("프로필 사진을 저장하지 못했습니다.");
        return;
      }
      setAvatarCropFile(null);
      setAvatarEditorOpen(false);
      setAvatarStatus("");
    } catch (error) {
      setAvatarStatus(error instanceof Error ? error.message : "프로필 사진 저장 실패");
    } finally {
      avatarSubmissionInFlight.current = false;
    }
  }

  if (guestProfile && (guestProfile.expired || guestAwaitingAdmission)) {
    return (
      <div className="dc-user-panel" ref={rootRef}>
        <div className="dc-current-user">
          <div className="dc-user-identity" aria-label="게스트 프로필">
            <span className="relative shrink-0">
              <span
                className="dc-self-avatar"
                data-has-image={guestHasAvatarImage}
                style={
                  guestHasAvatarImage
                    ? { backgroundImage: `url(${guestProfile?.avatarImage})` }
                    : undefined
                }
              >
                {guestHasAvatarImage ? null : guestAvatarLabel}
              </span>
              <span
                className={`dc-self-status ${guestProfile.expired ? "offline" : "online"}`}
                aria-hidden
              />
            </span>
            <span className="min-w-0 flex-1 text-left">
              <span className="block truncate text-[14px] font-bold leading-5 text-text-primary">
                {guestDisplayName}
              </span>
              <span className="block truncate text-[12px] leading-4 text-text-muted">
                {guestProfile.statusLabel}
              </span>
            </span>
          </div>
          {guestProfile.expired && onGuestExit && (
            <div className="dc-user-actions">
              <button
                type="button"
                aria-label="게스트 화면 나가기"
                title="게스트 화면 나가기"
                data-danger
                onClick={onGuestExit}
              >
                <LogOut size={18} />
              </button>
            </div>
          )}
        </div>
      </div>
    );
  }

  if (!profileHydrated) {
    return (
      <div className="dc-user-panel" ref={rootRef}>
        <div className="dc-current-user">
          <div
            className="dc-user-identity"
            aria-label={profileError ? "프로필 불러오기 실패" : "프로필 불러오는 중"}
            role="status"
          >
            <span className="dc-self-avatar" aria-hidden>
              …
            </span>
            <span className="min-w-0 flex-1 text-left">
              <span className="block truncate text-[14px] font-bold leading-5 text-text-primary">
                {profileError || "프로필 불러오는 중"}
              </span>
              <span className="block truncate text-[12px] leading-4 text-text-muted">
                {profileError ? "서버 사용자 정보 확인 실패" : "서버 사용자 정보 확인 중"}
              </span>
            </span>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div
      className="dc-user-panel"
      ref={rootRef}
      style={profileCssVars(profile, profileSnapshot.displayResourceBase)}
    >
      {profileOpen && (
        <section
          className="dc-profile-card"
          aria-label="내 프로필 카드"
        >
          <div
            className="dc-profile-banner"
            data-preset={profile.bannerPreset}
            style={profileCssVars(profile, profileSnapshot.displayResourceBase)}
          />
          <button
            type="button"
            className="dc-profile-close"
            onClick={() => {
              setProfileOpen(false);
              setSettingsOpen(false);
            }}
            aria-label="프로필 닫기"
          >
            <X size={16} />
          </button>
          <button
            type="button"
            className="dc-profile-avatar-wrap"
            onClick={openAvatarEditor}
            aria-label="프로필 사진 편집"
          >
            <span className="dc-profile-avatar" data-has-image={hasAvatarImage}>
              {hasAvatarImage ? null : profile.avatarLabel}
              <span className="dc-profile-avatar-edit" aria-hidden>
                <Pencil size={22} />
              </span>
            </span>
            <span className={`dc-profile-status ${statusClass}`} aria-hidden />
          </button>
          <div className="dc-profile-body">
            <div className="dc-profile-card-title">
              <div>
                <h2>{profile.displayName}</h2>
              </div>
            </div>
            <p>{profile.handle}</p>
            <div className="dc-profile-badges">
              {profile.customStatus && <span>{profile.customStatus}</span>}
              <span>#room-client</span>
            </div>
            <button
              type="button"
              className="dc-profile-status-row"
              onClick={() => openSettings("profile")}
            >
              <span className="dc-profile-status-add">+</span>
              <span>
                <strong>사용자 지정 상태</strong>
                <small>{profile.customStatus || "방금 플레이를 마쳤어요..."}</small>
              </span>
            </button>
            <div className="dc-profile-card-actions">
              <button type="button" onClick={() => openSettings("profile")}>
                <UserPen size={15} />
                프로필 편집
              </button>
              <button
                type="button"
                onClick={() => {
                  setProfileOpen(false);
                  setSettingsOpen(false);
                }}
              >
                <X size={15} />
                닫기
              </button>
            </div>
            {profileError && (
              <p className="dc-profile-notice" role="status">
                {profileError}
              </p>
            )}
            <div className="dc-profile-room-summary" aria-label="방 접속 요약">
              <span>방 접속 요약</span>
              <strong>{onlineCount}명 온라인</strong>
              <small>{agentCount}명 참가자/에이전트 표시 중</small>
            </div>
            <div className="dc-profile-menu">
              <button
                type="button"
                aria-expanded={statusMenuOpen}
                onClick={() => setStatusMenuOpen((value) => !value)}
              >
                <span className={`dc-profile-menu-dot ${statusClass}`} aria-hidden />
                내 상태: {profileStatusLabel(profile.status)}
                <ChevronDown size={16} />
              </button>
              {statusMenuOpen && (
                <div className="dc-profile-status-options" aria-label="빠른 상태 변경">
                  {PROFILE_STATUS_OPTIONS.map((option) => (
                    <button
                      key={option.id}
                      type="button"
                      className="dc-profile-status-option"
                      data-status={option.id}
                      aria-pressed={profile.status === option.id}
                      onClick={() => setProfileStatus(option.id)}
                    >
                      <span className={`dc-profile-menu-dot ${option.id}`} aria-hidden />
                      <span>
                        <strong>{option.label}</strong>
                        <small>{option.helper}</small>
                      </span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          </div>
        </section>
      )}

      {avatarEditorOpen && (
        <section
          className="dc-profile-avatar-modal"
          role="dialog"
          aria-modal="true"
          aria-label="프로필 사진 수정"
          onClick={(event) => event.stopPropagation()}
        >
          <header>
            <h2>프로필 사진 수정</h2>
            <button
              type="button"
              className="dc-modal-close"
              onClick={() => {
                setAvatarEditorOpen(false);
                setAvatarCropFile(null);
              }}
              aria-label="프로필 사진 수정 닫기"
            >
              <X size={18} />
            </button>
          </header>
          <label className="dc-profile-avatar-upload">
            이미지 선택
            <input
              type="file"
              accept="image/*"
              onChange={(event) => {
                const file = event.currentTarget.files?.[0] || null;
                if (file) setAvatarCropFile(file);
                event.currentTarget.value = "";
              }}
            />
          </label>
          {avatarCropFile ? (
            <ImageCropper
              file={avatarCropFile}
              onCancel={() => setAvatarCropFile(null)}
              onCropped={(file) => void handleAvatarCropped(file)}
            />
          ) : (
            <p className="dc-profile-notice">얼굴이 중앙에 오도록 이미지를 선택한 뒤 확대/위치를 조정하세요.</p>
          )}
          {avatarStatus && <p className="dc-profile-notice">{avatarStatus}</p>}
        </section>
      )}

      {settingsOpen && (
        <section className="dc-profile-settings-modal" role="dialog" aria-modal="true" aria-label="사용자 설정">
          <header className="dc-profile-settings-header">
            <div>
              <span>내 계정</span>
              <h2>사용자 설정</h2>
            </div>
            <button
              type="button"
              className="dc-profile-settings-close"
              onClick={() => setSettingsOpen(false)}
              aria-label="사용자 설정 닫기"
            >
              <X size={18} />
            </button>
          </header>
          <UserSettingsPanel
            draft={draft}
            saving={saving}
            profileError={profileError}
            settingsSection={settingsSection}
            onSectionChange={setSettingsSection}
            onDraftChange={setDraft}
            onReset={() => setDraft(profile)}
            onSave={() => void saveDraft()}
            onEditAvatar={openAvatarEditor}
            profileIdentity={profileIdentity}
            displayResourceBase={profileSnapshot.displayResourceBase}
          />
        </section>
      )}

      <div className="dc-current-user">
        <button
          type="button"
          className="dc-user-identity"
          onClick={openProfile}
          aria-expanded={profileOpen}
        >
          <span className="relative shrink-0">
            <span className="dc-self-avatar" data-has-image={hasAvatarImage}>
              {hasAvatarImage ? null : profile.avatarLabel}
            </span>
            <span className={`dc-self-status ${statusClass}`} aria-hidden />
          </span>
          <span className="min-w-0 flex-1 text-left">
            <span className="block truncate text-[14px] font-bold leading-5 text-text-primary">
              {profile.displayName}
            </span>
            <span className="block truncate text-[12px] leading-4 text-text-muted">
              {profileStatusLabel(profile.status)}
            </span>
          </span>
        </button>
        <div className="dc-user-actions">
          <button
            type="button"
            aria-label={profile.micMuted ? "마이크 음소거 해제" : "마이크 음소거"}
            aria-pressed={profile.micMuted}
            data-danger={profile.micMuted}
            className="dc-user-action-primary"
            onClick={() => updateProfileFlag("micMuted", !profile.micMuted)}
          >
            {profile.micMuted ? <MicOff size={16} /> : <Mic size={16} />}
          </button>
          <button
            type="button"
            aria-label="마이크 옵션"
            data-danger={profile.micMuted}
            className="dc-user-action-caret"
            onClick={() => openSettings("voice")}
          >
            <ChevronDown size={14} />
          </button>
          <button
            type="button"
            aria-label={profile.deafened ? "헤드셋 켜기" : "헤드셋 끄기"}
            aria-pressed={profile.deafened}
            onClick={() => updateProfileFlag("deafened", !profile.deafened)}
          >
            <Headphones size={16} />
          </button>
          <button
            type="button"
            aria-label="오디오 옵션"
            className="dc-user-action-caret"
            onClick={() => openSettings("voice")}
          >
            <ChevronDown size={14} />
          </button>
          <button type="button" aria-label="사용자 설정" onClick={() => openSettings("account")}>
            <Settings size={16} />
          </button>
        </div>
      </div>
    </div>
  );
}
