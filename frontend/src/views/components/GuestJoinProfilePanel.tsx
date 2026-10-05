import { useState, type ReactNode } from "react";
import { LogIn, RotateCcw } from "lucide-react";
import { fileToBase64 } from "../../api/http";
import type { OperatorPairingState } from "../../app/useRoomAdmission";
import ImageCropper from "./ImageCropper";

type GuestJoinProfilePanelProps = {
  children?: ReactNode;
  title?: string;
  titleContent?: ReactNode;
  identityLabel?: string;
  serverLabel?: string;
  roomLabel?: string;
  onMemberJoin?: () => void;
  displayName: string;
  avatarImage?: string;
  status?: string;
  busy?: boolean;
  pairing?: boolean;
  pairingState?: OperatorPairingState;
  retryMode?: "preflight" | "join";
  onDisplayNameChange: (value: string) => void;
  onAvatarImageChange: (value: string) => void;
  onJoin: () => void;
  onPairingRetry?: () => void;
};

export default function GuestJoinProfilePanel({
  children,
  title,
  titleContent,
  identityLabel,
  serverLabel,
  roomLabel,
  onMemberJoin,
  displayName,
  avatarImage,
  status = "",
  busy = false,
  pairing = false,
  pairingState = "idle",
  retryMode,
  onDisplayNameChange,
  onAvatarImageChange,
  onJoin,
  onPairingRetry,
}: GuestJoinProfilePanelProps) {
  const [cropFile, setCropFile] = useState<File | null>(null);
  const [uploadStatus, setUploadStatus] = useState("");
  const [avatarPreparing, setAvatarPreparing] = useState(false);
  const avatarLabel = (displayName || "G").slice(0, 1).toUpperCase() || "G";

  async function handleCropped(file: File) {
    if (avatarPreparing) return;
    setAvatarPreparing(true);
    setUploadStatus("프로필 사진 준비 중...");
    try {
      if (file.type !== "image/png" || file.size > 1_400_000) {
        throw new Error("프로필 사진이 너무 큽니다.");
      }
      onAvatarImageChange(`data:image/png;base64,${await fileToBase64(file)}`);
      setCropFile(null);
      setUploadStatus("입장할 때 사진이 저장됩니다.");
    } catch (error) {
      setUploadStatus(error instanceof Error ? error.message : "프로필 사진 준비 실패");
    } finally {
      setAvatarPreparing(false);
    }
  }

  const heading = title || (pairing ? "운영자 기기 연결" : children ? "로그인하고 참가"
    : retryMode ? "입장 확인" : roomLabel ? `‘${roomLabel}’에 초대받았어요` : "참가를 준비하고 있어요");

  return (
    <div className="dc-guest-join-panel">
      <section
        className="dc-guest-join-card"
        aria-label={
          pairing
            ? "운영자 기기 연결"
            : retryMode
            ? retryMode === "preflight" ? "입장 확인 재시도" : "입장 재시도"
            : heading
        }
      >
        {(identityLabel || (!children && !pairing && !retryMode && roomLabel)) &&
          <span className="dc-join-identity-icon" aria-hidden="true">{(identityLabel || roomLabel || "").slice(0, 1).toUpperCase()}</span>}
        <h1>{titleContent || heading}</h1>
        {serverLabel && <p className="dc-join-server-label">{serverLabel}</p>}
        {!children && !pairing && !retryMode && (
          <div className="dc-guest-avatar-row">
            <label className="dc-guest-avatar" data-has-image={Boolean(avatarImage)}>
              {avatarImage ? <img src={avatarImage} alt="" /> : <span aria-hidden="true">{avatarLabel}</span>}
              <input
                className="sr-only"
                type="file"
                aria-label="프로필 사진"
                accept="image/*"
                onChange={(event) => {
                  const file = event.currentTarget.files?.[0] || null;
                  if (file) setCropFile(file);
                  event.currentTarget.value = "";
                }}
              />
            </label>
            <label className="dc-guest-name-field">
              이름
              <input
                type="text"
                maxLength={80}
                value={displayName}
                onChange={(event) => onDisplayNameChange(event.currentTarget.value)}
                placeholder="방에서 보일 이름"
              />
            </label>
          </div>
        )}
        {pairing && pairingState === "pairing_failed_retryable" && (
          <button
            type="button"
            className="dc-guest-join-button"
            disabled={busy}
            onClick={onPairingRetry}
          >
            <RotateCcw size={16} />
            다시 시도
          </button>
        )}
        {pairing && pairingState === "pairing_failed_terminal" && (
          <a className="dc-guest-join-button" href="/">
            새 연결 링크 받기
          </a>
        )}
        {retryMode && (
          <button
            type="button"
            className="dc-guest-join-button"
            disabled={busy}
            onClick={onJoin}
          >
            <RotateCcw size={16} />
            다시 시도
          </button>
        )}
        {!children && !pairing && !retryMode && cropFile && (
          <ImageCropper
            file={cropFile}
            onCancel={() => setCropFile(null)}
            onCropped={(file) => void handleCropped(file)}
          />
        )}
        {!children && !pairing && !retryMode && (
          <>
            {onMemberJoin && <button type="button" className="dc-guest-join-button"
              disabled={busy || avatarPreparing} onClick={onMemberJoin}>
              <LogIn size={16} /> 로그인하고 참가
            </button>}
            <button
              type="button"
              className="dc-member-session-button"
              data-active="false"
              disabled={busy || avatarPreparing || !displayName.trim()}
              onClick={onJoin}
            >
              <LogIn size={16} />
              게스트로 참가
            </button>
            {onMemberJoin && <p className="text-sm text-text-muted">로그인하면 다른 기기에서도 같은 사람으로 참가할 수 있어요.</p>}
          </>
        )}
        {children}
        {(status || uploadStatus) && (
          <p role={retryMode ? "alert" : "status"} className="dc-member-session-status preserve-words">{uploadStatus || status}</p>
        )}
      </section>
    </div>
  );
}
