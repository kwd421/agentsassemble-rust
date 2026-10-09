import { useResourceImage } from "./ResourceImage";
import { resolveAttachmentReference } from "../../lib/attachmentReference";
import { useEffect, useRef, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  Headphones,
  LogOut,
  Mic,
  MicOff,
  Pencil,
  Plus,
  Settings,
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
import ImageCropDialog from "./ImageCropDialog";
import UserSettingsPanel, { type UserSettingsSection } from "./UserSettingsPanel";

export default function UserPanel({
  onlineCount,
  agentCount,
  hasBackendError,
  guestProfile,
  pairedRoomSession = false,
  profileAuthorityReady = true,
  profileIdentity = {},
  publishedProfileRevision = 0,
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
  pairedRoomSession?: boolean;
  profileAuthorityReady?: boolean;
  profileIdentity?: UserProfileIdentity;
  publishedProfileRevision?: number;
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
  const [profileSnapshot, setProfileSnapshot] = useState<UserProfileSnapshot | null>(null);
  const profile = profileSnapshot?.profile ?? initialProfile;
  const displayResourceBase = profileSnapshot?.displayResourceBase || "";
  const profileAvatar = useResourceImage(resolveAttachmentReference(profile.avatarImage, displayResourceBase));
  const [draft, setDraft] = useState<UserProfile>(initialProfile);
  const [profileOpen, setProfileOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsSection, setSettingsSection] = useState<UserSettingsSection>("account");
  const [statusMenuOpen, setStatusMenuOpen] = useState(false);
  const [avatarCropFile, setAvatarCropFile] = useState<File | null>(null);
  const [avatarStatus, setAvatarStatus] = useState("");
  const [saving, setSaving] = useState(false);
  const [profileError, setProfileError] = useState("");
  const [profileHydrated, setProfileHydrated] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const settingsButtonRef = useRef<HTMLButtonElement>(null);
  const avatarInputRef = useRef<HTMLInputElement>(null);
  // The crop dialog is the avatar editor; it opens once a file has been picked.
  const avatarEditorOpen = avatarCropFile !== null;
  const profileSnapshotRef = useRef<UserProfileSnapshot | null>(null);
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
  const guestAwaitingAdmission = Boolean(guestProfile && !profileIdentity.centralSession && !profileIdentity.sessionToken);

  useEffect(() => {
    const generation = ++profileScopeGeneration.current;
    profileIntentGeneration.current += 1;
    profileWriteGeneration.current += 1;
    setSaving(false);
    if (!profileAuthorityReady || guestProfile?.expired || guestAwaitingAdmission || pairedRoomSession) {
      setProfileError("");
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
            : "프로필을 불러오지 못했어요."
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
    profileAuthorityReady,
    guestProfile?.expired,
    guestAwaitingAdmission,
    pairedRoomSession,
    profileIdentity.deviceToken,
    profileIdentity.sessionToken,
    profileIdentity.centralSession?.sessionToken,
    profileIdentity.centralSession?.generation,
  ]);

  useEffect(() => {
    // Let an open editor retain its draft and revision. Its save still detects
    // concurrent writes; closing it refreshes from the committed server profile.
    if (!profileHydrated || pairedRoomSession || guestProfile?.expired || saving ||
        settingsOpen || profileOpen || avatarEditorOpen ||
        publishedProfileRevision <= (profileSnapshot?.revision ?? 0)) return;
    let current = true;
    const generation = profileScopeGeneration.current;
    const refresh = profileWriteTail.current.then(async () => {
      if (!current || generation !== profileScopeGeneration.current) return;
      try {
        const loaded = await fetchUserProfile(profileIdentity);
        if (!current || generation !== profileScopeGeneration.current) return;
        if (loaded.revision > (profileSnapshotRef.current?.revision ?? 0)) {
          profileSnapshotRef.current = loaded;
          setProfileSnapshot(loaded);
          setDraft(loaded.profile);
        }
        setProfileError("");
      } catch (error) {
        if (current && generation === profileScopeGeneration.current) {
          setProfileError(error instanceof Error ? error.message : "프로필 변경을 확인하지 못했어요.");
        }
      }
    });
    profileWriteTail.current = refresh;
    return () => { current = false; };
  }, [publishedProfileRevision, profileSnapshot?.revision, profileHydrated,
    pairedRoomSession, guestProfile?.expired, saving, settingsOpen, profileOpen,
    avatarEditorOpen, profileIdentity.deviceToken, profileIdentity.sessionToken,
    profileIdentity.centralSession?.sessionToken, profileIdentity.centralSession?.generation]);

  useEffect(() => {
    if (!profileOpen && !settingsOpen && !avatarEditorOpen) return;
    function closeOnOutside(event: MouseEvent) {
      if (!rootRef.current?.contains(event.target as Node)) {
        setProfileOpen(false);
        setSettingsOpen(false);
        setStatusMenuOpen(false);
        if (!avatarSubmissionInFlight.current) setAvatarCropFile(null);
      }
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") {
        setProfileOpen(false);
        setSettingsOpen(false);
        setStatusMenuOpen(false);
        if (!avatarSubmissionInFlight.current) setAvatarCropFile(null);
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

  // Like Discord, changing the photo goes straight to the file picker; the crop
  // dialog opens only after a file is chosen.
  function openAvatarEditor() {
    setProfileOpen(false);
    setAvatarStatus("");
    avatarInputRef.current?.click();
  }

  async function enqueueProfileOperation(
    execute: (currentSnapshot: UserProfileSnapshot) => Promise<UserProfileSnapshot | null>
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
        const currentSnapshot = profileSnapshotRef.current;
        if (!currentSnapshot) return "stale";
        setProfileError("");
        try {
          const savedSnapshot = await execute(currentSnapshot);
          if (!savedSnapshot) return "stale";
          if (profileScopeGeneration.current !== scopeGeneration) return "stale";
          profileSnapshotRef.current = savedSnapshot;
          setProfileSnapshot(savedSnapshot);
          setDraft(savedSnapshot.profile);
          return "saved";
        } catch (error) {
          if (profileScopeGeneration.current !== scopeGeneration) return "stale";
          const message =
            error instanceof Error ? error.message : "프로필을 저장하지 못했어요.";
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
                : "서버 프로필을 다시 확인하지 못했어요.";
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
    return enqueueProfileOperation((currentSnapshot) =>
      saveUserProfile(
        applyMutation(currentSnapshot.profile),
        currentSnapshot.revision,
        profileIdentity
      )
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
    const avatarScopeGeneration = profileScopeGeneration.current;
    setAvatarStatus("프로필 사진 저장 중...");
    try {
      const result = await enqueueProfileOperation(async (currentSnapshot) => {
        const avatarImage = await uploadUserProfileAvatar(file, profileIdentity);
        if (profileScopeGeneration.current !== avatarScopeGeneration) return null;
        return saveUserProfile(
          { ...currentSnapshot.profile, avatarImage },
          currentSnapshot.revision,
          profileIdentity
        );
      });
      if (result === "stale") return;
      if (result === "failed") {
        setAvatarStatus("프로필 사진을 저장하지 못했어요.");
        return;
      }
      setAvatarCropFile(null);
      setAvatarStatus("");
    } catch (error) {
      setAvatarStatus(error instanceof Error ? error.message : "프로필 사진 저장 실패");
    } finally {
      avatarSubmissionInFlight.current = false;
    }
  }

  const guestAvatar = useResourceImage(guestProfile?.avatarImage).url;

  if (guestProfile && (guestProfile.expired || guestAwaitingAdmission || pairedRoomSession)) {
    return (
      <div className="dc-user-panel" ref={rootRef}>
        <div className="dc-current-user">
          <div
            className="dc-user-identity"
            aria-label={pairedRoomSession ? "운영자 방 접속 프로필" : "게스트 프로필"}
            title={pairedRoomSession ? "계정과 프로필은 호스트 앱에서 변경해요." : undefined}
          >
            <span className="relative shrink-0">
              <span
                className="dc-self-avatar"
                data-has-image={guestHasAvatarImage}
                style={
                  guestHasAvatarImage
                    ? { backgroundImage: `url(${guestAvatar})` }
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
                {profileError ? "프로필을 불러오지 못했어요" : "프로필을 불러오고 있어요"}
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
      style={profileCssVars(profile, profileAvatar.url)}
    >
      {profileOpen && (
        <section
          className="dc-profile-card"
          aria-label="내 프로필 카드"
          style={{ left: 8, maxWidth: "calc(100% - 16px)" }}
        >
          <div
            className="dc-profile-banner"
            data-preset={profile.bannerPreset}
            style={profileCssVars(profile, profileAvatar.url)}
          />
          {profileAvatar.error && <p role="alert">{profileAvatar.error}</p>}
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
          {/* Discord puts the custom status in a bubble beside the avatar. */}
          <button
            type="button"
            className="dc-profile-status-bubble"
            data-empty={!profile.customStatus}
            onClick={() => openSettings("profile")}
            aria-label={profile.customStatus ? `사용자 지정 상태: ${profile.customStatus}` : "사용자 지정 상태 추가하기"}
          >
            {profile.customStatus ? (
              <span className="preserve-words">{profile.customStatus}</span>
            ) : (
              <>
                <Plus size={14} aria-hidden />
                <span>상태 추가하기</span>
              </>
            )}
          </button>
          <div className="dc-profile-body">
            <h2 className="dc-profile-name preserve-words">{profile.displayName}</h2>
            <p className="dc-profile-handle preserve-words">{profile.handle}</p>
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
            <div className="dc-profile-menu-card">
              <button type="button" onClick={() => openSettings("profile")}>
                <Pencil size={16} aria-hidden />
                <span>프로필 편집</span>
              </button>
              <span className="dc-profile-menu-card-separator" aria-hidden />
              <button
                type="button"
                aria-expanded={statusMenuOpen}
                onClick={() => setStatusMenuOpen((value) => !value)}
              >
                <span className={`dc-profile-menu-dot ${statusClass}`} aria-hidden />
                <span>{profileStatusLabel(profile.status)}</span>
                <ChevronRight size={16} aria-hidden className="dc-profile-menu-card-chevron" />
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

      <input
        ref={avatarInputRef}
        type="file"
        accept="image/*"
        hidden
        aria-label="이미지 선택"
        onChange={(event) => {
          const file = event.currentTarget.files?.[0] || null;
          event.currentTarget.value = "";
          if (file) {
            setAvatarStatus("");
            setAvatarCropFile(file);
          }
        }}
      />
      {avatarCropFile && (
        <ImageCropDialog
          title="프로필 사진 수정"
          file={avatarCropFile}
          shape="circle"
          busy={saving}
          status={avatarStatus}
          onCancel={() => {
            setAvatarCropFile(null);
            setAvatarStatus("");
          }}
          onApply={(file) => void handleAvatarCropped(file)}
        />
      )}

      {settingsOpen && (
          <UserSettingsPanel
            returnFocusRef={settingsButtonRef}
            onClose={() => setSettingsOpen(false)}
            changed={JSON.stringify(draft) !== JSON.stringify(profile)}
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
            displayResourceBase={displayResourceBase}
          />
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
          <button ref={settingsButtonRef} type="button" aria-label="사용자 설정" onClick={() => openSettings("account")}>
            <Settings size={16} />
          </button>
        </div>
      </div>
    </div>
  );
}
