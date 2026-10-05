import { PUBLIC_INGRESS_START_ERROR } from "../../lib/publicIngressStatus";
import { QRCodeSVG } from "qrcode.react";
import type { SavedFriendsAuthority } from "../../api/friends";
import { useFriendsDirectory } from "../../app/useFriendsDirectory";
import { AttendeeFriendInviteCard, type AttendeeInviteControls } from "./AttendeeFriendInviteCard";
import { ConnectorInviteCard, type ConnectorInviteControls } from "./ConnectorInviteCard";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Copy, Settings, X } from "lucide-react";
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

const HUMAN_INVITE_TTL_LABELS: Record<number, string> = { 3600: "1시간", 86400: "24시간", 604800: "7일" };

function humanInviteSummary(maxUses: number, ttlSeconds: number) {
  const uses = maxUses === 0 ? "인원 제한 없이" : maxUses === 1 ? "1명만" : `${maxUses}명까지`;
  return `${uses} 쓸 수 있고 ${HUMAN_INVITE_TTL_LABELS[ttlSeconds] || `${ttlSeconds}초`} 후 만료돼요.`;
}

export default function RoomInviteModal({
  roomLabel,
  friendAuthority,
  canControlIngress = true,
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
  publicAccessQuery,
  onRetryPublicAccess,
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
  friendAuthority?: SavedFriendsAuthority;
  canControlIngress?: boolean;
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
  publicAccessQuery: PublicAccessQuery;
  onRetryPublicAccess?: () => void;
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
  const publicAccessKnown = publicAccessQuery === "confirmed";
  const publicAccessRunning = publicAccessKnown && Boolean(publicUrl || tunnelStatus?.public_url);
  const publicTunnelActive = Boolean(tunnelStatus?.running);
  const publicAccessControllable = Boolean(tunnelStatus?.available);
  const publicAccessBusy = publicAccessStarting || publicAccessStopping || !publicAccessKnown;
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
    onGenerateSecureInvite(action.options, true);
  }

  return (
    <div className="dc-modal-backdrop" role="presentation" onClick={onClose}>
      <dialog
        ref={dialogRef}
        className="dc-invite-modal"
        style={{ margin: "auto", maxHeight: "calc(100dvh - 32px)", color: "var(--color-text-primary)" }}
        onCancel={(event) => { event.preventDefault(); onClose(); }}
        aria-modal="true"
        aria-labelledby="room-invite-title"
        onClick={(event) => { event.stopPropagation(); if (event.target === event.currentTarget) onClose(); }}
      >
        <header className="dc-invite-header">
          <h2 id="room-invite-title" className="preserve-words">
            {roomLabel}에 초대하기
          </h2>
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

        {/* Results belong next to the header: the panel below scrolls away from them. */}
        <p className="dc-invite-status" role="status" aria-live="polite" data-shown={Boolean(copyStatus)}>
          {copyStatus}
        </p>

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
            <SavedFriendInvitePicker directory={friendsDirectory} onSelect={setFriendDisplayName} />
            <p className="dc-invite-lead">초대받은 사람은 브라우저에서 이 링크로 참가해요.</p>
            <div className="dc-invite-link-field">
              <input
                className="dc-invite-link-input"
                value={secureInviteReady ? "초대 링크가 준비됐어요" : ""}
                placeholder={!publicAccessKnown ? "외부 접속 상태를 확인해 주세요" : publicAccessRunning ? "링크를 만들면 바로 복사할 수 있어요" : "링크를 만들 때 외부 접속을 함께 열어요"}
                readOnly
                aria-label="사람 초대 링크"
              />
              {secureInviteReady && selectedHumanInvite ? (
                <button
                  type="button"
                  className="dc-invite-copy-button"
                  style={{ minWidth: 44, minHeight: 44 }}
                  aria-label="현재 사람 초대 링크 복사"
                  onClick={() => onCopyHumanInvite(selectedHumanInvite.key)}
                >
                  <Copy size={15} />
                  복사
                </button>
              ) : (
                <button
                  type="button"
                  className="dc-invite-copy-button"
                  style={{ minWidth: 44, minHeight: 44 }}
                  aria-label="사람 초대 링크 생성"
                  disabled={publicAccessBusy}
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
                    style={{ minHeight: 44, appearance: "none" }}
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
                    style={{ minHeight: 44, appearance: "none" }}
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
                            className="dc-invite-row-button"
                            style={{ minWidth: 44, minHeight: 44 }}
                            aria-label={`사람 초대 ${index + 1} 링크 복사`}
                            disabled={!invite.copyUrl}
                            onClick={() => onCopyHumanInvite(invite.key)}
                          >
                            복사
                          </button>
                          <button
                            type="button"
                            className="dc-invite-row-button"
                            data-tone="danger"
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
          </section>}

          {tab === "ai" && attendeeInvites && <AttendeeFriendInviteCard directory={friendsDirectory} controls={attendeeInvites} disabled={publicAccessBusy || !publicAccessRunning} />}
          {tab === "ai" && connectorInvites && <ConnectorInviteCard controls={connectorInvites} localOnly={!publicAccessRunning}
            mcpOrigin={publicAccessBusy ? undefined : publicUrl}
            disabled={publicAccessBusy || (!publicAccessRunning && !tunnelStatus?.local_url)} />}
          {tab === "device" && onCreatePairing && onCopyPairing && onRevokePairing && (
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
                      <button type="button" className="dc-invite-row-button"
                        style={{ minWidth: 44, minHeight: 44, flexShrink: 0 }}
                        aria-label={`기기 연결 ${index + 1} 링크 복사`}
                        disabled={!pairing.copyable} onClick={() => onCopyPairing(pairing.key)}>복사</button>
                      <button type="button" className="dc-invite-row-button" data-tone="danger"
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

        <footer
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
          {publicAccessQuery === "unavailable" && onRetryPublicAccess && <button type="button" className="dc-invite-row-button" onClick={onRetryPublicAccess}>상태 다시 확인</button>}
          {canControlIngress && publicAccessKnown && (showStopTunnel ? (
            <button
              type="button"
              className="dc-invite-row-button"
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
          ) : (
            <button
              type="button"
              className="dc-invite-row-button"
              style={{ minWidth: 44, minHeight: 44 }}
              disabled={publicAccessBusy || !publicAccessControllable}
              onClick={onStartTunnel}
            >
              외부 접속 열기
            </button>
          ))}
          {!canControlIngress && <span className="dc-invite-hosting-note">호스트 기기에서 접속을 관리해요.</span>}
        </footer>
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
