import { useEffect, useRef, useState } from "react";
import { challengeRoomMember, joinRoomMember, type RoomInviteJoinResponse } from "../../api/invites";
import { ApiError } from "../../lib/apiErrors";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { createSecureRequestId } from "../../lib/secureRequestId";
import {
  centralAccountEntryUrl, issueCentralMemberGrant, loadCentralSession,
  loginCentralGoogle, previewCentralMember,
} from "../../lib/central/identity";
import { completeCentralWebGoogleReturn, startCentralWebGoogle } from "../../lib/central/webGoogle";
import {
  centralMemberEntryUrl, clearCentralMemberRequest, createMemberHandoff, memberCallbackUrl,
  memberRetryUrl, memberTargetRequest, storeMemberHandoff,
  type MemberGrant, type MemberHandoff, type MemberReturn, type MemberTargetRequest,
} from "../../lib/central/memberConnect";
import GuestJoinProfilePanel from "./GuestJoinProfilePanel";

export type MemberJoinHost = {
  inviteToken: string; meetingId: string; deviceToken: string; clientId: string;
  callback?: MemberReturn; onComplete: (payload: RoomInviteJoinResponse) => Promise<boolean>;
};
type Consent = {
  request: MemberTargetRequest; target: Awaited<ReturnType<typeof previewCentralMember>>;
  account: string; sessionToken: string;
};

function failureMessage(error: unknown): string {
  if (error instanceof ApiError) {
    const messages: Record<string, string> = {
      admission_session_unavailable: "이 방에는 다시 참가할 수 없어요. 방 관리자에게 문의해 주세요.",
      idempotency_conflict: "이 초대로는 참가할 수 없어요. 방 관리자에게 새 초대를 받아 주세요.",
      participant_identity_conflict: "참가자 정보를 확인하지 못했어요. 방 관리자에게 문의해 주세요.",
      member_challenge_invalid: "참가 요청을 사용할 수 없어요. 다시 시도해 주세요.",
      member_redeem_failed: "계정을 확인하지 못했어요. 다시 시도해 주세요.",
      invite_invalid: "초대나 방을 사용할 수 없어요. 방 관리자에게 문의해 주세요.",
      token_expired: "초대가 만료됐어요. 새 초대를 받아 주세요.",
      invite_revoked: "취소된 초대예요. 새 초대를 받아 주세요.",
    };
    return messages[error.code] || "참가할 수 없어요. 다시 시도해 주세요.";
  }
  if (error instanceof TypeError) return "서버에 연결할 수 없어요. 연결을 확인하고 다시 시도해 주세요.";
  // Only exact user-facing messages may cross the native/parser error boundary.
  const messages = [
    "참가 요청이 만료됐어요. 다시 시도해 주세요.",
    "참가를 완료하지 못했어요. 다시 시도해 주세요.",
    "참가 요청이 없어요. 원래 초대 링크를 다시 열어 주세요.",
    "로그인 계정이 바뀌었어요. 다시 시도해 주세요.",
    "서버 주소가 변경됐어요. 다시 시도해 주세요.",
  ];
  return error instanceof Error && messages.includes(error.message)
    ? error.message : "참가를 준비하지 못했어요. 초대 링크에서 다시 시도해 주세요.";
}

export default function MemberJoinPanel({ host, request, entryError, onCancel }: {
  host?: MemberJoinHost; request?: MemberTargetRequest; entryError?: string; onCancel?: () => void;
}) {
  const [consent, setConsent] = useState<Consent | null>(null);
  const [busy, setBusy] = useState(!entryError);
  const [status, setStatus] = useState("참가를 준비하고 있어요");
  const [error, setError] = useState(entryError ? failureMessage(new Error(entryError)) : "");
  const record = useRef<MemberHandoff | undefined>(host?.callback?.record);
  const active = useRef(true);
  const flight = useRef(false);
  const initialized = useRef(false);
  const loginAbort = useRef<AbortController | null>(null);

  async function run(action: () => Promise<void>) {
    if (flight.current) return;
    flight.current = true; setBusy(true); setError("");
    try { await action(); }
    catch (reason) { if (active.current) setError(failureMessage(reason)); }
    finally { flight.current = false; if (active.current) setBusy(false); }
  }

  async function finish(pending: MemberHandoff, grant: MemberGrant) {
    if (!host || !active.current) return;
    if (pending.expires_at <= Date.now() / 1000 || grant.expires_at <= Date.now() / 1000) {
      throw new Error("참가 요청이 만료됐어요. 다시 시도해 주세요.");
    }
    const payload = await joinRoomMember(pending, grant.grant_token, createSecureRequestId(), host.clientId, host.deviceToken);
    if (!active.current) return;
    if (payload.server_id !== pending.server_id) throw new Error("참가를 완료하지 못했어요. 다시 시도해 주세요.");
    if (!(await host.onComplete(payload))) throw new Error("참가를 완료하지 못했어요. 다시 시도해 주세요.");
  }

  async function prepare() {
    setConsent(null);
    let targetRequest = request;
    if (host) {
      setStatus("참가를 준비하고 있어요");
      const challenge = await challengeRoomMember(host.inviteToken, host.deviceToken);
      if (!active.current) return;
      record.current = createMemberHandoff(challenge, host.inviteToken, host.meetingId);
      targetRequest = memberTargetRequest(record.current);
      if (!isDesktopWebview()) {
        const central = centralAccountEntryUrl();
        if (!central) throw new Error("참가를 준비하지 못했어요. 초대 링크에서 다시 시도해 주세요.");
        const url = centralMemberEntryUrl(new URL(central).origin, targetRequest);
        storeMemberHandoff(record.current);
        setStatus("참가를 준비하고 있어요");
        window.location.assign(url);
        return;
      }
      if (!loadCentralSession()) {
        loginAbort.current = new AbortController();
        await loginCentralGoogle(() => { if (active.current) setStatus("참가를 준비하고 있어요"); }, loginAbort.current.signal);
      }
    } else {
      await completeCentralWebGoogleReturn();
    }
    if (!active.current) return;
    if (!targetRequest) throw new Error("참가 요청이 없어요. 원래 초대 링크를 다시 열어 주세요.");
    const session = loadCentralSession();
    if (!session) { setStatus("로그인하면 참가를 이어갈 수 있어요."); return; }
    const target = await previewCentralMember(targetRequest);
    if (!active.current) return;
    if (loadCentralSession()?.token !== session.token) throw new Error("로그인 계정이 바뀌었어요. 다시 시도해 주세요.");
    setConsent({ request: targetRequest, target, account: session.person.display_name, sessionToken: session.token });
    setStatus("");
  }

  useEffect(() => {
    active.current = true;
    if (!initialized.current) {
      initialized.current = true;
      if (!entryError) queueMicrotask(() => { if (active.current) void run(async () => {
        const callback = host?.callback;
        if (callback?.error) throw new Error(callback.error);
        if (callback?.record && callback.grant) await finish(callback.record, callback.grant);
        else await prepare();
      }); });
    }
    const returned = (event: PageTransitionEvent) => {
      if (event.persisted && host && !isDesktopWebview()) setError("참가를 완료하지 않았어요. 다시 시도해 주세요.");
    };
    window.addEventListener("pageshow", returned);
    return () => { active.current = false; loginAbort.current?.abort(); window.removeEventListener("pageshow", returned); };
    // This component owns one selected invite/return. Explicit retries use prepare().
  }, []);

  async function confirm() {
    if (!consent || busy) return;
    await run(async () => {
      setStatus("참가를 준비하고 있어요");
      if (loadCentralSession()?.token !== consent.sessionToken) throw new Error("로그인 계정이 바뀌었어요. 다시 시도해 주세요.");
      if (record.current && record.current.expires_at <= Date.now() / 1000) throw new Error("참가 요청이 만료됐어요. 다시 시도해 주세요.");
      const grant = await issueCentralMemberGrant(consent.request);
      if (!active.current) return;
      if (grant.endpoint_origin !== consent.target.endpoint_origin || grant.endpoint_generation !== consent.target.endpoint_generation) {
        throw new Error("서버 주소가 변경됐어요. 다시 시도해 주세요.");
      }
      if (host && record.current) await finish(record.current, grant);
      else {
        clearCentralMemberRequest();
        window.location.assign(memberCallbackUrl(grant, consent.request));
      }
    });
  }

  function retry() {
    if (host) { void run(prepare); return; }
    clearCentralMemberRequest();
    if (consent) window.location.assign(memberRetryUrl(consent.target.endpoint_origin, consent.request));
    else window.history.back();
  }

  return <GuestJoinProfilePanel displayName="" busy={busy} status={error || status}
    title={consent && !error ? `${consent.target.label}에 참가할까요?` : "로그인하고 참가"}
    identityLabel={consent && !error ? consent.target.label : undefined}
    titleContent={consent && !error ? <><strong>{consent.target.label}</strong>에 참가할까요?</> : undefined}
    retryMode={error ? "join" : undefined} onJoin={retry}
    onDisplayNameChange={() => {}} onAvatarImageChange={() => {}}>
    <section aria-label="서버 참가" className="grid gap-3 text-text-primary">
      {consent && !error && <>
        <p className="text-sm preserve-words">{consent.account} 계정으로 참가</p>
        <p className="truncate text-xs text-text-muted">{consent.target.endpoint_origin}</p>
        <p className="text-sm text-text-muted">참가하면 이 서버에 내 이름과 프로필이 보여요.</p>
        <button type="button" className="dc-guest-join-button" disabled={busy} onClick={() => void confirm()}>참가하기</button>
      </>}
      {!host && !busy && !error && !consent && <button type="button" className="dc-guest-join-button"
        onClick={() => void run(async () => {
          loginAbort.current = new AbortController();
          await startCentralWebGoogle(loginAbort.current.signal);
        })}>Google로 계속</button>}
      {(!host || onCancel) && !busy && <button type="button" className="dc-join-cancel"
        onClick={onCancel || (() => { clearCentralMemberRequest(); window.history.back(); })}>취소</button>}
    </section>
  </GuestJoinProfilePanel>;
}
