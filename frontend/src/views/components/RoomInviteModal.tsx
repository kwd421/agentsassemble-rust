import { PUBLIC_INGRESS_START_ERROR } from "../../lib/publicIngressStatus";
import { hostPublishQuotaNotice } from "../../lib/central/centralRequestGovernor";
import { QRCodeSVG } from "qrcode.react";
import type { SavedFriendsAuthority } from "../../api/friends";
import { useFriendsDirectory } from "../../app/useFriendsDirectory";
import { AttendeeFriendInviteCard, type AttendeeInviteControls } from "./AttendeeFriendInviteCard";
import { ConnectorInviteCard, type ConnectorInviteControls } from "./ConnectorInviteCard";
import type { ConnectorSeat } from "../../app/useManagedAiInvites";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Check, Copy, Settings, X } from "../../ui/icons";
import type { PublicInviteStatus } from "../../api";
import type {
  HumanInviteOptions,
  PublicAccessTransition,
  PublicAccessQuery,
} from "../../app/useRoomInviteController";
import type { HumanInvitePresentation } from "../../app/useManagedHumanInvites";
import type { OperatorPairingPresentation } from "../../app/useManagedOperatorPairings";
import type { RoomAppearance } from "../../lib/roomAppearance";
import SavedFriendInvitePicker from "./SavedFriendInvitePicker";
import { humanInviteSummary } from "../../lib/humanInviteSummary";

type PendingPublicAction = { kind: "human"; options: HumanInviteOptions };

type InviteTabId = "people" | "ai" | "device";

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

function inviteExpiryLabel(value: string) {
  const expiry = new Date(value);
  if (!Number.isFinite(expiry.getTime())) return "만료 시각 확인 불가";
  const now = new Date();
  const tomorrow = new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1);
  const day = expiry.toDateString() === now.toDateString() ? "오늘"
    : expiry.toDateString() === tomorrow.toDateString() ? "내일"
    : expiry.toLocaleDateString("ko-KR", { ...(expiry.getFullYear() !== now.getFullYear() ? { year: "numeric" as const } : {}), month: "long", day: "numeric" });
  return `${day} ${expiry.toLocaleTimeString("ko-KR", { hour: "numeric", minute: "2-digit", hour12: true })} 만료`;
}


export default function RoomInviteModal({
  roomLabel,
  friendAuthority,
  canControlIngress = true,
  showHosting = true,
  canCreateLink = true,
  humanInvites = [],
  connectorInvites,
  connectorSeats,
  attendeeInvites,
  operatorPairings = [],
  pairingCreating = false,
  onCreatePairing,
  onCopyPairing,
  onRevokePairing,
  publicUrl,
  publicAccessTransition = "idle",
  publicAccessQuery,
  onRetryPublicAccess,
  tunnelStatus,
  quotaParkedUntil,
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
  /** `null`: this person has no saved friends here, so friend pickers stay hidden. */
  friendAuthority?: SavedFriendsAuthority | null;
  canControlIngress?: boolean;
  /** False for a friend's room: this computer's external access has nothing to do with it. */
  showHosting?: boolean;
  /** False in a room this computer neither hosts nor manages as its owner (a friend's room). */
  canCreateLink?: boolean;
  humanInvites?: readonly HumanInvitePresentation[];
  connectorInvites?: ConnectorInviteControls;
  /** External AIs of this room that a reconnect invitation can put back in their seat. */
  connectorSeats?: readonly ConnectorSeat[];
  attendeeInvites?: AttendeeInviteControls;
  operatorPairings?: readonly OperatorPairingPresentation[];
  pairingCreating?: boolean;
  onCreatePairing?: () => void;
  onCopyPairing?: (key: string) => boolean | Promise<boolean> | void;
  onRevokePairing?: (key: string) => void;
  publicUrl?: string;
  publicAccessTransition?: PublicAccessTransition;
  publicAccessQuery: PublicAccessQuery;
  onRetryPublicAccess?: () => void;
  tunnelStatus?: PublicInviteStatus["tunnel"];
  quotaParkedUntil?: number | null;
  inviteScope?: RoomAppearance["inviteScope"];
  copyStatus?: string;
  onClose: () => void;
  onGenerateSecureInvite: (options: HumanInviteOptions, startTunnelIfNeeded: boolean) => void;
  onCopyHumanInvite: (key: string) => boolean | Promise<boolean> | void;
  onRevokeHumanInvite: (key: string) => void;
  onStartTunnel: () => void;
  onStopTunnel: () => void;
}) {
  const friendsDirectory = useFriendsDirectory(friendAuthority);
  const dialogRef = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  useEffect(() => {
    const dialog = dialogRef.current; dialog?.showModal();
    return () => { dialog?.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [opener]);
  const [humanMaxUses, setHumanMaxUses] = useState(1);
  const [tab, setTab] = useState<InviteTabId>("people");
  const [friendDisplayName, setFriendDisplayName] = useState<string>();
  const [humanTtlSeconds, setHumanTtlSeconds] = useState(86400);
  const [linkSettingsOpen, setLinkSettingsOpen] = useState(false);
  const [pendingPublicAction, setPendingPublicAction] =
    useState<PendingPublicAction | null>(null);
  // Copy feedback lives on the pressed button ("복사됨" for ~2 s), never in a box.
  const [copiedKey, setCopiedKey] = useState("");
  const copiedTimer = useRef<number | undefined>(undefined);
  useEffect(() => () => {
    if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
  }, []);
  function flashCopied(key: string) {
    setCopiedKey(key);
    if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
    copiedTimer.current = window.setTimeout(() => setCopiedKey(""), 2000);
  }
  function handleCopy(key: string, copy: (key: string) => boolean | Promise<boolean> | void) {
    void Promise.resolve(copy(key)).then((ok) => {
      if (ok === true) flashCopied(key);
    });
  }
  const readOnlyInvite = inviteScope === "read_only";
  const currentHumanOptions = { maxUses: humanMaxUses, ttlSeconds: humanTtlSeconds, ...(friendDisplayName ? { displayName: friendDisplayName } : {}) };
  const selectedHumanInvite = humanInvites.find(
    (invite) =>
      !invite.retired &&
      (invite.displayName === "Guest" ? "게스트" : invite.displayName) === (friendDisplayName ?? "게스트") &&
      invite.maxUses === humanMaxUses &&
      invite.ttlSeconds === humanTtlSeconds
  );
  const secureInviteReady = Boolean(selectedHumanInvite?.copyUrl);
  const publicAccessStarting =
    publicAccessTransition === "starting" || tunnelStatus?.phase === "starting";
  const publicAccessStopping =
    publicAccessTransition === "stopping" || tunnelStatus?.phase === "stopping";
  const publicAccessKnown = publicAccessQuery === "confirmed";
  const publicAccessRunning = publicAccessKnown && Boolean(publicUrl || tunnelStatus?.public_url);
  const publicTunnelActive = Boolean(tunnelStatus?.running);
  const publicAccessControllable = Boolean(tunnelStatus?.available);
  const publicAccessBusy = publicAccessStarting || publicAccessStopping || !publicAccessKnown;
  const quotaParkedNotice = quotaParkedUntil ? hostPublishQuotaNotice(quotaParkedUntil) : null;
  // One hosting control at a time: an open or opening tunnel offers only Stop.
  const showStopTunnel =
    publicAccessStarting || publicAccessStopping || publicTunnelActive || publicAccessRunning;
  function requestPublicAction(action: PendingPublicAction) {
    if (publicAccessRunning) {
      onGenerateSecureInvite(action.options, false);
      return;
    }
    setPendingPublicAction(action);
  }

  const canPairDevices = Boolean(onCreatePairing && onCopyPairing && onRevokePairing);
  const tabs: Array<{ id: InviteTabId; label: string }> = [
    { id: "people", label: "사람" },
    ...(attendeeInvites || connectorInvites ? [{ id: "ai" as const, label: "AI" }] : []),
    ...(canPairDevices ? [{ id: "device" as const, label: "내 기기" }] : []),
  ];

  function confirmPublicAction() {
    const action = pendingPublicAction;
    setPendingPublicAction(null);
    if (!action) return;
    onGenerateSecureInvite(action.options, canControlIngress);
  }

  return (
    <div className="dc-modal-backdrop" role="presentation" onClick={onClose}>
      <dialog
        ref={dialogRef}
        className="dc-dialog dc-dialog--flush dc-invite-modal"
        onCancel={(event) => { event.preventDefault(); onClose(); }}
        aria-modal="true"
        aria-labelledby="room-invite-title"
        onClick={(event) => { event.stopPropagation(); if (event.target === event.currentTarget) onClose(); }}
      >
        <header className="dc-invite-header">
          <h2 id="room-invite-title" className="dc-dialog-title preserve-words">
            ‘{roomLabel}’에 초대하기
          </h2>
          <button
            type="button"
            className="dc-dialog-close"
            onClick={onClose}
            aria-label="초대 닫기"
          >
            <X size={24} />
          </button>
        </header>

        {/* Errors only: success needs no box (the button shows "복사됨",
            toggles show their own state, new links appear in the list). */}
        {copyStatus && (
          <p className="dc-invite-status" data-tone="error" role="alert" aria-live="polite">
            {copyStatus}
          </p>
        )}

        {tabs.length > 1 && (
          <div className="dc-invite-tabs" role="tablist" aria-label="초대 종류">
            {tabs.map((entry) => (
              <button
                key={entry.id}
                type="button"
                role="tab"
                id={`invite-tab-${entry.id}`}
                aria-selected={tab === entry.id}
                aria-controls={`invite-panel-${entry.id}`}
                data-active={tab === entry.id}
                onClick={() => setTab(entry.id)}
              >
                {entry.label}
              </button>
            ))}
          </div>
        )}

        <div
          className="dc-invite-primary-grid"
          role="tabpanel"
          id={`invite-panel-${tab}`}
          aria-labelledby={tabs.length > 1 ? `invite-tab-${tab}` : "room-invite-title"}
        >
          {tab === "people" && <section className="dc-invite-people" aria-label="사람 초대">
            {!canCreateLink ? <p className="dc-invite-lead">초대 링크는 방 주인만 만들 수 있어요.</p> : <>
            {friendAuthority !== null && <SavedFriendInvitePicker directory={friendsDirectory} onSelect={setFriendDisplayName} />}
            <p className="dc-invite-lead">초대받은 사람은 브라우저에서 이 링크로 참가해요.</p>
            <div className="dc-invite-link-field">
              <input
                className="dc-invite-link-input"
                value={secureInviteReady ? "초대 링크가 준비됐어요" : ""}
                placeholder={!publicAccessKnown ? "외부 접속 상태를 확인해 주세요" : publicAccessRunning ? "링크를 만들면 바로 복사할 수 있어요" : canControlIngress ? "링크를 만들 때 외부 접속을 함께 열어요" : "서버 주인이 외부 접속을 열면 링크를 만들 수 있어요"}
                readOnly
                aria-label="사람 초대 링크"
              />
              {secureInviteReady && selectedHumanInvite ? (
                <button
                  type="button"
                  className="dc-button dc-button--primary"
                  aria-label={copiedKey === selectedHumanInvite.key ? "현재 사람 초대 링크 복사됨" : "현재 사람 초대 링크 복사"}
                  onClick={() => handleCopy(selectedHumanInvite.key, onCopyHumanInvite)}
                >
                  {copiedKey === selectedHumanInvite.key ? <Check size={15} /> : <Copy size={15} />}
                  {copiedKey === selectedHumanInvite.key ? "복사됨" : "복사"}
                </button>
              ) : (
                <button
                  type="button"
                  className="dc-button dc-button--primary"
                  aria-label="사람 초대 링크 생성"
                  disabled={publicAccessBusy || (!publicAccessRunning && !canControlIngress)}
                  onClick={() => requestPublicAction({ kind: "human", options: currentHumanOptions })}
                >
                  링크 만들기
                </button>
              )}
            </div>
            <div className="dc-invite-summary-row">
              <p>
                {humanInviteSummary(humanMaxUses, humanTtlSeconds)}
                {readOnlyInvite ? " 읽기 전용으로 참가해요." : ""}
              </p>
              <button
                type="button"
                className="dc-invite-icon-button"
                aria-label="링크 설정"
                title="링크 설정"
                aria-expanded={linkSettingsOpen}
                aria-controls="human-invite-settings"
                onClick={() => setLinkSettingsOpen((open) => !open)}
              >
                <Settings size={18} />
              </button>
            </div>
            {linkSettingsOpen && (
              <div className="dc-invite-options" id="human-invite-settings">
                <label>
                  <span>초대 가능 인원</span>
                  <select
                    className="dc-select"
                    style={{ minHeight: 44 }}
                    value={humanMaxUses}
                    onChange={(event) => setHumanMaxUses(Number(event.currentTarget.value))}
                  >
                    <option value={1}>1명</option>
                    <option value={5}>5명</option>
                    <option value={0}>제한 없음</option>
                  </select>
                </label>
                <label>
                  <span>링크 유효시간</span>
                  <select
                    className="dc-select"
                    style={{ minHeight: 44 }}
                    value={humanTtlSeconds}
                    onChange={(event) => setHumanTtlSeconds(Number(event.currentTarget.value))}
                  >
                    <option value={3600}>1시간</option>
                    <option value={86400}>24시간</option>
                    <option value={604800}>7일</option>
                  </select>
                </label>
              </div>
            )}
            </>}
            {humanInvites.length > 0 && (
              <div className="dc-invite-issued" aria-label="발급한 사람 초대">
                <span className="dc-invite-issued-title">만든 링크</span>
                <div className="grid gap-1" role="list">
                  {humanInvites.map((invite, index) => {
                    const revokeBusy = invite.revocation === "in_flight";
                    const revokeDead = invite.revocation === "dead";
                    return (
                      <div className="dc-invite-friend-row" style={{ flexWrap: "wrap" }} role="listitem" key={invite.key}>
                        <span className="min-w-0 flex-1" style={{ flexBasis: 160 }}>
                          <span className="dc-invite-friend-name preserve-words">
                            {invite.displayName === "Guest" ? "게스트" : invite.displayName}
                          </span>
                          <span className="dc-invite-friend-handle preserve-words">
                            {humanInviteUseLabel(invite.maxUses)} · {inviteExpiryLabel(invite.expiresAt)} ·{" "}
                            {humanInviteStatus(invite)}
                          </span>
                        </span>
                        <span className="flex shrink-0 items-center gap-2">
                          <button
                            type="button"
                            className="dc-button dc-button--lg dc-button--secondary"
                            style={{ minWidth: 44 }}
                            aria-label={`사람 초대 ${index + 1} 링크 ${copiedKey === invite.key ? "복사됨" : "복사"}`}
                            disabled={!invite.copyUrl}
                            onClick={() => handleCopy(invite.key, onCopyHumanInvite)}
                          >
                            {copiedKey === invite.key ? "복사됨" : "복사"}
                          </button>
                          <button
                            type="button"
                            className="dc-button dc-button--lg dc-button--danger-soft"
                            style={{ minWidth: 44 }}
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
          </section>}

          {tab === "ai" && attendeeInvites && friendAuthority !== null && <AttendeeFriendInviteCard directory={friendsDirectory} controls={attendeeInvites} disabled={publicAccessBusy || !publicAccessRunning} />}
          {tab === "ai" && connectorInvites && <ConnectorInviteCard controls={connectorInvites} localOnly={!publicAccessRunning} seats={connectorSeats}
            mcpOrigin={publicAccessBusy ? undefined : publicUrl}
            disabled={publicAccessBusy || (!publicAccessRunning && !tunnelStatus?.local_url)} />}
          {tab === "device" && onCreatePairing && onCopyPairing && onRevokePairing && (
            <section className="dc-invite-card" aria-labelledby="operator-pairing-heading">
              <div>
                <h3 id="operator-pairing-heading">내 기기 연결</h3>
                <p>내 다른 기기에서 이 방을 운영할 수 있어요. 연결 링크는 다른 사람에게 보내지 마세요.</p>
              </div>
              <button type="button" className="dc-button dc-button--lg dc-button--primary"
                aria-label="운영자 기기 연결 링크 생성"
                disabled={pairingCreating || publicAccessBusy || !publicAccessRunning}
                onClick={onCreatePairing}>
                {pairingCreating ? "링크 만드는 중" : "연결 링크 만들기"}
              </button>
              {publicAccessKnown && !publicAccessRunning && <p>외부 접속을 연 뒤 연결할 수 있어요.</p>}
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
                      {pairing.copyable && !pairing.expired && pairing.state === "ready" && pairing.qrUrl && (
                        <div className="dc-pairing-qr">
                          <QRCodeSVG value={pairing.qrUrl} size={192} marginSize={4} level="M"
                            role="img" aria-label={`기기 연결 ${index + 1} QR 코드`} />
                          <p>휴대폰 카메라로 스캔해 연결하세요.</p>
                        </div>
                      )}
                      <button type="button" className="dc-button dc-button--lg dc-button--secondary"
                        style={{ minWidth: 44, flexShrink: 0 }}
                        aria-label={`기기 연결 ${index + 1} 링크 ${copiedKey === pairing.key ? "복사됨" : "복사"}`}
                        disabled={!pairing.copyable} onClick={() => handleCopy(pairing.key, onCopyPairing)}>{copiedKey === pairing.key ? "복사됨" : "복사"}</button>
                      <button type="button" className="dc-button dc-button--lg dc-button--danger-soft"
                        style={{ minWidth: 44, flexShrink: 0 }}
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

        {showHosting && <footer
          className="dc-invite-hosting"
          data-state={publicAccessBusy ? "busy" : publicAccessRunning ? "public" : "local"}
          aria-labelledby="room-hosting-heading"
        >
          <span className="dc-invite-hosting-dot" aria-hidden="true" />
          <div className="dc-invite-hosting-copy">
            <h3 id="room-hosting-heading" className="sr-only">서버 외부 접속</h3>
            <p className="dc-invite-hosting-state">
              {publicAccessStarting
                ? "공개 준비 중"
                : publicAccessStopping
                  ? "외부 접속 닫는 중"
                  : publicAccessQuery === "checking"
                    ? "외부 접속 확인 중"
                    : publicAccessQuery === "unavailable"
                      ? "외부 접속 상태 확인 필요"
                      : publicAccessRunning && quotaParkedNotice
                    ? quotaParkedNotice
                    : publicAccessRunning
                    ? "외부 접속 열림"
                    : "외부 접속 꺼짐"}
            </p>
            <p className="dc-invite-hosting-detail">
              {!publicAccessKnown
                ? publicAccessQuery === "checking" ? "접속 상태를 확인하고 있어요." : "접속 상태를 불러오지 못했어요. 다시 확인해 주세요."
                : publicAccessRunning
                ? publicUrl || tunnelStatus?.public_url || "외부 주소가 연결되어 있어요."
                : "이 컴퓨터에서는 계속 대화할 수 있어요."}
            </p>
            {tunnelStatus?.last_error && (
              <p className="dc-invite-hosting-error preserve-words">{PUBLIC_INGRESS_START_ERROR}</p>
            )}
          </div>
          {publicAccessQuery === "unavailable" && onRetryPublicAccess && <button type="button" className="dc-button dc-button--secondary" onClick={onRetryPublicAccess}>상태 다시 확인</button>}
          {canControlIngress && publicAccessKnown && (showStopTunnel ? (
            <button
              type="button"
              className="dc-button dc-button--lg dc-button--secondary"
              style={{ minWidth: 44 }}
              disabled={
                publicAccessStopping ||
                (!publicAccessStarting && !publicTunnelActive) ||
                !publicAccessControllable
              }
              onClick={onStopTunnel}
            >
              외부 접속 끄기
            </button>
          ) : (
            <button
              type="button"
              className="dc-button dc-button--lg dc-button--secondary"
              style={{ minWidth: 44 }}
              disabled={publicAccessBusy || !publicAccessControllable}
              onClick={onStartTunnel}
            >
              외부 접속 열기
            </button>
          ))}
          {!canControlIngress && <span className="dc-invite-hosting-note">호스트 기기에서 접속을 관리해요.</span>}
        </footer>}
        {pendingPublicAction && (
          <PublicAccessConfirmation onCancel={() => setPendingPublicAction(null)}>
              <h3 id="public-access-confirm-title" className="dc-dialog-title">외부 접속을 열까요?</h3>
              <p>
                이 컴퓨터의 서버에 임시 공개 주소를 연결한 뒤 사람 초대 링크를 만들어요. 링크를 가진 사람만 참가할 수 있어요.
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
  return <dialog ref={ref} className="dc-dialog dc-invite-confirm" role="alertdialog" aria-labelledby="public-access-confirm-title"
    onCancel={(event) => { event.preventDefault(); event.stopPropagation(); onCancel(); }}
    onClick={(event) => { event.stopPropagation(); if (event.target === event.currentTarget) onCancel(); }}>{children}</dialog>;
}
