import { useFriendsDirectory } from "../../app/useFriendsDirectory";
import { AttendeeFriendInviteCard, type AttendeeInviteControls } from "./AttendeeFriendInviteCard";
import { ConnectorInviteCard, type ConnectorInviteControls } from "./ConnectorInviteCard";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Copy, Globe2, LoaderCircle, LockKeyhole, X } from "lucide-react";
import type { PublicInviteStatus } from "../../api";
import type {
  HumanInviteOptions,
  PublicAccessTransition,
} from "../../app/useRoomInviteController";
import type { HumanInvitePresentation } from "../../app/useManagedHumanInvites";
import type { OperatorPairingPresentation } from "../../app/useManagedOperatorPairings";
import type { RoomAppearance } from "../../lib/roomAppearance";
import SavedFriendInvitePicker from "./SavedFriendInvitePicker";

type PendingPublicAction = { kind: "human"; options: HumanInviteOptions };

function humanInviteStatus(invite: HumanInvitePresentation) {
  if (invite.revocation === "dead") return "폐기됨";
  if (invite.revocation === "in_flight") return "폐기 중";
  if (invite.revocation === "unknown") return "폐기 결과 미확인";
  if (invite.expired) return "만료됨";
  if (invite.retired) return "이전 초대";
  if (!invite.originCurrent) return "공개 주소 변경됨";
  if (!invite.authorityCurrent) return "방 권위 변경됨";
  return invite.copyUrl ? "복사 가능" : "폐기만 가능";
}

function humanInviteUseLabel(maxUses: number) {
  return maxUses === 0 ? "인원 제한 없음" : `${maxUses}명`;
}

export default function RoomInviteModal({
  roomLabel,
  humanInvites = [],
  connectorInvites,
  attendeeInvites,
  operatorPairings = [],
  pairingCreating = false,
  onCreatePairing,
  onCopyPairing,
  onRevokePairing,
  publicUrl,
  publicAccessTransition = "idle",
  tunnelStatus,
  inviteScope = "room",
  copyStatus,
  onClose,
  onGenerateSecureInvite,
  onCopyHumanInvite,
  onRevokeHumanInvite,
  onStartTunnel,
  onStopTunnel,
}: {
  roomLabel: string;
  humanInvites?: readonly HumanInvitePresentation[];
  connectorInvites?: ConnectorInviteControls;
  attendeeInvites?: AttendeeInviteControls;
  operatorPairings?: readonly OperatorPairingPresentation[];
  pairingCreating?: boolean;
  onCreatePairing?: () => void;
  onCopyPairing?: (key: string) => void;
  onRevokePairing?: (key: string) => void;
  publicUrl?: string;
  publicAccessTransition?: PublicAccessTransition;
  tunnelStatus?: PublicInviteStatus["tunnel"];
  inviteScope?: RoomAppearance["inviteScope"];
  copyStatus?: string;
  onClose: () => void;
  onGenerateSecureInvite: (options: HumanInviteOptions, startTunnelIfNeeded: boolean) => void;
  onCopyHumanInvite: (key: string) => void;
  onRevokeHumanInvite: (key: string) => void;
  onStartTunnel: () => void;
  onStopTunnel: () => void;
}) {
  const friendsDirectory = useFriendsDirectory();
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  useEffect(() => {
    const dialog = dialogRef.current; dialog?.showModal();
    return () => { dialog?.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [opener]);
  const [humanMaxUses, setHumanMaxUses] = useState(1);
  const [friendDisplayName, setFriendDisplayName] = useState<string>();
  const [humanTtlSeconds, setHumanTtlSeconds] = useState(86400);
  const [pendingPublicAction, setPendingPublicAction] =
    useState<PendingPublicAction | null>(null);
  const readOnlyInvite = inviteScope === "read_only";
  const currentHumanOptions = { maxUses: humanMaxUses, ttlSeconds: humanTtlSeconds, ...(friendDisplayName ? { displayName: friendDisplayName } : {}) };
  const selectedHumanInvite = humanInvites.find(
    (invite) =>
      !invite.retired &&
      invite.displayName === (friendDisplayName ?? "Guest") &&
      invite.maxUses === humanMaxUses &&
      invite.ttlSeconds === humanTtlSeconds
  );
  const secureInviteReady = Boolean(selectedHumanInvite?.copyUrl);
  const publicAccessStarting =
    publicAccessTransition === "starting" || tunnelStatus?.phase === "starting";
  const publicAccessStopping =
    publicAccessTransition === "stopping" || tunnelStatus?.phase === "stopping";
  const publicAccessRunning = Boolean(publicUrl || tunnelStatus?.public_url);
  const publicTunnelActive = Boolean(tunnelStatus?.running);
  const publicAccessControllable = Boolean(tunnelStatus?.available);
  const publicAccessBusy = publicAccessStarting || publicAccessStopping;
  function requestPublicAction(action: PendingPublicAction) {
    if (publicAccessRunning) {
      onGenerateSecureInvite(action.options, false);
      return;
    }
    setPendingPublicAction(action);
  }

  function confirmPublicAction() {
    const action = pendingPublicAction;
    setPendingPublicAction(null);
    if (!action) return;
    onGenerateSecureInvite(action.options, true);
  }

  return (
    <div className="dc-modal-backdrop" role="presentation" onClick={onClose}>
      <dialog
        ref={dialogRef}
        className="dc-invite-modal"
        style={{ margin: "auto", padding: 24, maxHeight: "calc(100dvh - 32px)", color: "var(--color-text-primary)" }}
        onCancel={(event) => { event.preventDefault(); onClose(); }}
        aria-modal="true"
        aria-labelledby="room-invite-title"
        onClick={(event) => { event.stopPropagation(); if (event.target === event.currentTarget) onClose(); }}
      >
        <header className="flex items-start justify-between gap-4">
          <div className="min-w-0">
            <h2 id="room-invite-title" className="text-[18px] font-black text-text-primary preserve-words" style={{ overflowWrap: "anywhere" }}>
              {roomLabel} 초대 및 연결
            </h2>
            <p className="mt-1 text-[13px] text-text-muted preserve-words">
              사람과 AI를 초대하거나 내 다른 기기를 연결해요.
            </p>
          </div>
          <button
            type="button"
            className="dc-modal-close"
            style={{ minWidth: 44, minHeight: 44, flexShrink: 0 }}
            onClick={onClose}
            aria-label="초대 닫기"
          >
            <X size={18} />
          </button>
        </header>

        <section
          className="dc-invite-hosting"
          data-state={publicAccessBusy ? "busy" : publicAccessRunning ? "public" : "local"}
          aria-labelledby="room-hosting-heading"
        >
          <span className="dc-invite-hosting-icon" aria-hidden="true">
            {publicAccessBusy ? (
              <LoaderCircle className="dc-invite-hosting-spinner" size={22} />
            ) : publicAccessRunning ? (
              <Globe2 size={22} />
            ) : (
              <LockKeyhole size={22} />
            )}
          </span>
          <div className="dc-invite-hosting-copy">
            <div className="dc-invite-hosting-title-row">
              <h3 id="room-hosting-heading">이 컴퓨터의 서버 공개</h3>
              <span className="dc-invite-hosting-state">
                {publicAccessStarting
                  ? "공개 준비 중"
                  : publicAccessStopping
                    ? "외부 접속 닫는 중"
                    : publicAccessRunning
                      ? "외부 접속 열림"
                      : "외부 접속 꺼짐"}
              </span>
            </div>
            <p style={{ whiteSpace: "normal", overflow: "visible", overflowWrap: "anywhere" }}>
              {publicAccessRunning
                ? publicUrl || tunnelStatus?.public_url || "외부 주소가 연결되어 있어요."
                : "외부 접속을 꺼도 이 컴퓨터에서는 계속 대화할 수 있어요."}
            </p>
            {tunnelStatus?.last_error && (
              <span className="mt-1 text-[12px] font-bold text-offline preserve-words">
                {tunnelStatus.last_error}
              </span>
            )}
          </div>
          <div className="dc-invite-hosting-actions">
            <button
              type="button"
              className="dc-invite-copy-button"
              style={{ minWidth: 44, minHeight: 44 }}
              disabled={
                publicAccessBusy ||
                publicAccessRunning ||
                publicTunnelActive ||
                !publicAccessControllable
              }
              onClick={onStartTunnel}
            >
              외부 접속 열기
            </button>
            <button
              type="button"
              className="dc-invite-copy-button"
              style={{ minWidth: 44, minHeight: 44 }}
              disabled={
                publicAccessStopping ||
                (!publicAccessStarting && !publicTunnelActive) ||
                !publicAccessControllable
              }
              onClick={onStopTunnel}
            >
              외부 접속 끄기
            </button>
          </div>
        </section>

        <div className="dc-invite-primary-grid">
          <section className="dc-invite-card" aria-labelledby="human-invite-heading">
            <div>
              <h3 id="human-invite-heading">사람 초대</h3>
              <p>
                초대받은 사람은 브라우저에서 참가해요.
                {readOnlyInvite ? " 이 방에서는 읽기 전용으로 참가합니다." : ""}
              </p>
            </div>
            <div className="dc-invite-options">
              <SavedFriendInvitePicker directory={friendsDirectory} onSelect={setFriendDisplayName} />
              <label>
                <span>초대 가능 인원</span>
                <select
                  style={{ minHeight: 44, appearance: "none" }}
                  value={humanMaxUses}
                  onChange={(event) => setHumanMaxUses(Number(event.currentTarget.value))}
                >
                  <option value={1}>1명 (권장)</option>
                  <option value={5}>5명</option>
                  <option value={0}>제한 없음</option>
                </select>
              </label>
              <label>
                <span>링크 유효시간</span>
                <select
                  style={{ minHeight: 44, appearance: "none" }}
                  value={humanTtlSeconds}
                  onChange={(event) => setHumanTtlSeconds(Number(event.currentTarget.value))}
                >
                  <option value={3600}>1시간</option>
                  <option value={86400}>24시간 (권장)</option>
                  <option value={604800}>7일</option>
                </select>
              </label>
            </div>
            <div className="dc-invite-link-row">
              <input
                className="dc-invite-link-input"
                value={secureInviteReady ? "보안 초대 링크 발급됨" : ""}
                placeholder="공개 주소를 준비하면 링크가 표시됩니다"
                readOnly
                aria-label="사람 초대 링크"
              />
              <button
                type="button"
                className="dc-invite-copy-button"
                style={{ minWidth: 44, minHeight: 44 }}
                aria-label="사람 초대 링크 생성"
                onClick={() => requestPublicAction({ kind: "human", options: currentHumanOptions })}
              >
                생성
              </button>
              <button
                type="button"
                className="dc-invite-copy-button"
                style={{ minWidth: 44, minHeight: 44 }}
                aria-label="현재 사람 초대 링크 복사"
                disabled={!secureInviteReady || !selectedHumanInvite}
                onClick={() => {
                  if (selectedHumanInvite) onCopyHumanInvite(selectedHumanInvite.key);
                }}
              >
                <Copy size={15} />
                복사
              </button>
            </div>
            {humanInvites.length > 0 && (
              <div className="dc-invite-setup" aria-label="발급한 사람 초대">
                <span className="text-[12px] font-black text-text-secondary">
                  이 앱에서 발급한 링크
                </span>
                <div className="grid gap-2" role="list">
                  {humanInvites.map((invite, index) => {
                    const revokeBusy = invite.revocation === "in_flight";
                    const revokeDead = invite.revocation === "dead";
                    return (
                      <div className="dc-invite-friend-row" style={{ flexWrap: "wrap" }} role="listitem" key={invite.key}>
                        <span className="min-w-0 flex-1" style={{ flexBasis: 160 }}>
                          <span className="dc-invite-friend-name preserve-words">
                            {invite.displayName}
                          </span>
                          <span className="dc-invite-friend-handle preserve-words">
                            {humanInviteUseLabel(invite.maxUses)} · 만료 {invite.expiresAt} ·{" "}
                            {humanInviteStatus(invite)}
                          </span>
                        </span>
                        <span className="flex shrink-0 items-center gap-2">
                          <button
                            type="button"
                            className="dc-invite-copy-button"
                            style={{ minWidth: 44, minHeight: 44 }}
                            aria-label={`사람 초대 ${index + 1} 링크 복사`}
                            disabled={!invite.copyUrl}
                            onClick={() => onCopyHumanInvite(invite.key)}
                          >
                            <Copy size={14} />
                            복사
                          </button>
                          <button
                            type="button"
                            className="dc-invite-copy-button"
                            style={{ minWidth: 44, minHeight: 44 }}
                            aria-label={`사람 초대 ${index + 1} 폐기`}
                            disabled={revokeBusy || revokeDead}
                            onClick={() => onRevokeHumanInvite(invite.key)}
                          >
                            {revokeBusy
                              ? "폐기 중"
                              : revokeDead
                                ? "폐기됨"
                                : invite.revocation === "unknown"
                                  ? "폐기 재시도"
                                  : "폐기"}
                          </button>
                        </span>
                      </div>
                    );
                  })}
                </div>
              </div>
            )}
          </section>

          {attendeeInvites && <AttendeeFriendInviteCard directory={friendsDirectory} controls={attendeeInvites} disabled={publicAccessBusy || !publicAccessRunning} />}
          {connectorInvites && <ConnectorInviteCard controls={connectorInvites} disabled={publicAccessBusy || !publicAccessRunning} />}
          {onCreatePairing && onCopyPairing && onRevokePairing && (
            <section className="dc-invite-card" aria-labelledby="operator-pairing-heading">
              <div>
                <h3 id="operator-pairing-heading">내 기기 연결</h3>
                <p>내 다른 기기에서 이 방을 운영할 수 있어요. 연결 링크는 다른 사람에게 보내지 마세요.</p>
              </div>
              <button type="button" className="dc-invite-copy-button"
                style={{ minWidth: 44, minHeight: 44 }}
                aria-label="운영자 기기 연결 링크 생성"
                disabled={pairingCreating || publicAccessBusy || !publicAccessRunning}
                onClick={onCreatePairing}>
                {pairingCreating ? "링크 만드는 중" : "연결 링크 만들기"}
              </button>
              {!publicAccessRunning && <p>외부 접속을 연 뒤 연결할 수 있어요.</p>}
              {operatorPairings.length > 0 && (
                <div className="grid gap-2" role="list" aria-label="이 앱에서 발급한 기기 연결">
                  {operatorPairings.map((pairing, index) => (
                    <div className="dc-invite-friend-row" style={{ flexWrap: "wrap" }} role="listitem" key={pairing.key}>
                      <span className="min-w-0 flex-1" style={{ flexBasis: 160 }}>
                        <span className="dc-invite-friend-name">기기 연결 {index + 1}</span>
                        <span className="dc-invite-friend-handle preserve-words">
                          {pairing.state === "revoked" ? "연결 해제됨"
                            : pairing.state === "unknown" ? "연결 해제 결과 미확인"
                            : pairing.expired ? "링크 만료"
                            : pairing.copyable ? `링크 만료 ${new Date(pairing.expiresAt).toLocaleTimeString()}`
                            : "연결 해제만 가능"}
                        </span>
                      </span>
                      <button type="button" className="dc-invite-copy-button"
                        style={{ minWidth: 44, minHeight: 44, flexShrink: 0 }}
                        aria-label={`기기 연결 ${index + 1} 링크 복사`}
                        disabled={!pairing.copyable} onClick={() => onCopyPairing(pairing.key)}>복사</button>
                      <button type="button" className="dc-invite-copy-button"
                        style={{ minWidth: 44, minHeight: 44, flexShrink: 0 }}
                        aria-label={`기기 연결 ${index + 1} 해제`}
                        disabled={pairing.state === "revoking" || pairing.state === "revoked"}
                        onClick={() => onRevokePairing(pairing.key)}>
                        {pairing.state === "revoking" ? "해제 중" : pairing.state === "unknown" ? "해제 재시도" : "연결 해제"}
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </section>
          )}
        </div>

        <p className="mt-3 text-[12px] text-text-muted preserve-words" role="status">
          {copyStatus ||
            (readOnlyInvite
              ? "이 방의 사람 초대는 읽기 전용 권한으로 발급됩니다."
              : "초대 링크는 참가할 사람이나 AI 대화에만 전달해 주세요.")}
        </p>
        {pendingPublicAction && (
          <PublicAccessConfirmation onCancel={() => setPendingPublicAction(null)}>
              <h3 id="public-access-confirm-title">외부 접속을 열까요?</h3>
              <p>
                이 컴퓨터의 서버에 임시 공개 주소를 연결한 뒤 사람 초대 링크를 만듭니다. 링크를 가진 사람만 참가할 수 있습니다.
              </p>
              <div className="dc-invite-confirm-actions">
                <button
                  type="button"
                  className="dc-agent-create-secondary"
                  style={{ minWidth: 44, minHeight: 44 }}
                  autoFocus
                  onClick={() => setPendingPublicAction(null)}
                >
                  취소
                </button>
                <button
                  type="button"
                  className="dc-invite-confirm-primary"
                  style={{ minWidth: 44, minHeight: 44 }}
                  onClick={confirmPublicAction}
                >
                  외부 접속 열고 링크 만들기
                </button>
              </div>
          </PublicAccessConfirmation>
        )}
      </dialog>
    </div>
  );
}

function PublicAccessConfirmation({ onCancel, children }: { onCancel: () => void; children: ReactNode }) {
  const ref = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  useEffect(() => {
    const dialog = ref.current; dialog?.showModal();
    return () => { dialog?.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [opener]);
  return <dialog ref={ref} className="dc-invite-confirm" role="alertdialog" aria-labelledby="public-access-confirm-title"
    style={{ margin: "auto", width: "min(430px, calc(100vw - 32px))", maxHeight: "calc(100dvh - 32px)", overflowY: "auto", padding: 24 }}
    onCancel={(event) => { event.preventDefault(); event.stopPropagation(); onCancel(); }}
    onClick={(event) => { event.stopPropagation(); if (event.target === event.currentTarget) onCancel(); }}>{children}</dialog>;
}
