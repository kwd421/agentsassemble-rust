import { SecureMemberAdmission, previewSecureMember, type SecureMemberEntry } from "../../lib/central/secureMemberEntry";
import { loginMemberPopup } from "../../lib/central/memberPopup";
import { useEffect, useRef, useState } from "react";
import { challengeMemberConnect, redeemMemberConnect, selectMemberConnect, challengeRoomMember, joinRoomMember, type RoomInviteJoinResponse } from "../../api/invites";
import { ApiError } from "../../lib/apiErrors";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { createSecureRequestId } from "../../lib/secureRequestId";
import {
  centralAccountEntryUrl, issueCentralMemberGrant, loadCentralSession,
  loginCentralGoogle, previewCentralMember, setCentralMemberHidden,
} from "../../lib/central/identity";
import { completeCentralWebGoogleReturn, startCentralWebGoogle } from "../../lib/central/webGoogle";
import {
  centralMemberEntryUrl, clearCentralMemberRequest, createMemberHandoff, memberCallbackUrl,
  memberRetryUrl, memberTargetRequest, memberRoomName, storeMemberHandoff,
  type MemberGrant, type MemberHandoff, type MemberReturn, type MemberTargetRequest,
} from "../../lib/central/memberConnect";
import GuestJoinProfilePanel from "./GuestJoinProfilePanel";

export type MemberJoinHost = {
  purpose?: "connect";
  inviteToken: string; meetingId: string; roomName?: string; deviceToken: string; clientId: string;
  callback?: MemberReturn; onComplete: (payload: RoomInviteJoinResponse) => Promise<boolean>;
};
type Consent = {
  request: MemberTargetRequest; target: Awaited<ReturnType<typeof previewCentralMember>> & { target?: import("../../lib/remote/secureCrypto").SecureTarget };
  account: string; sessionToken: string;
};

function failureMessage(error: unknown): string {
  if (error instanceof ApiError) {
    const messages: Record<string, string> = {
      member_no_rooms: "이 서버에 참가 중인 방이 없어요.",
      member_temporarily_unavailable: "지금은 연결 요청이 많아요. 잠시 뒤 다시 시도해 주세요.",
      member_membership_ended: "이 방에는 다시 참가할 수 없어요. 방 관리자에게 문의해 주세요.",
      admission_session_unavailable: "참가를 마치지 못했어요. 다시 시도해 주세요.",
      idempotency_conflict: "이 초대로는 참가할 수 없어요. 방 관리자에게 새 초대를 받아 주세요.",
      participant_identity_conflict: "참가자 정보를 확인하지 못했어요. 방 관리자에게 문의해 주세요.",
      member_challenge_invalid: "참가 요청을 사용할 수 없어요. 다시 시도해 주세요.",
      member_redeem_failed: "참가를 마치지 못했어요. 다시 시도해 주세요.",
      invite_invalid: "초대나 방을 사용할 수 없어요. 방 관리자에게 문의해 주세요.",
      token_expired: "초대가 만료됐어요. 새 초대를 받아 주세요.",
      invite_revoked: "취소된 초대예요. 새 초대를 받아 주세요.",
    };
    return messages[error.code] || "참가를 마치지 못했어요. 다시 시도해 주세요.";
  }
  if (error instanceof TypeError) return "참가를 마치지 못했어요. 다시 시도해 주세요.";
  // Only exact user-facing messages may cross the native/parser error boundary.
  const messages = [
    "참가 요청이 만료됐어요. 다시 시도해 주세요.",
    "참가를 마치지 못했어요. 다시 시도해 주세요.",
    "참가 요청이 없어요. 원래 초대 링크를 다시 열어 주세요.",
    "로그인 계정이 바뀌었어요. 다시 시도해 주세요.",
    "서버 주소가 변경됐어요. 다시 시도해 주세요.",
  ];
  return error instanceof Error && messages.includes(error.message)
    ? error.message : "참가를 마치지 못했어요. 다시 시도해 주세요.";
}

export default function MemberJoinPanel({ host, request, entryError, onCancel, secureEntry }: {
  secureEntry?: SecureMemberEntry; host?: MemberJoinHost; request?: MemberTargetRequest; entryError?: string; onCancel?: () => void;
}) {
  const [rooms, setRooms] = useState<{room_id:string;name:string}[]>([]);
  const [consent, setConsent] = useState<Consent | null>(null);
  const [busy, setBusy] = useState(!entryError);
  const [status, setStatus] = useState("참가를 준비하고 있어요");
  const [error, setError] = useState(entryError ? failureMessage(new Error(entryError)) : "");
  const secureAdmission = useRef<SecureMemberAdmission | null>(null);
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
    if (pending.purpose === "connect") {
      const available = await redeemMemberConnect(pending, grant.grant_token, host.deviceToken);
      if (!active.current) return;
      setConsent(null); setStatus("");
      if (available.length === 1) await selectRoom(available[0].room_id, pending);
      else setRooms(available);
      return;
    }
    const payload = await joinRoomMember(pending, grant.grant_token, createSecureRequestId(), host.clientId, host.deviceToken);
    if (!active.current) return;
    if (payload.server_id !== pending.server_id) throw new Error("참가를 마치지 못했어요. 다시 시도해 주세요.");
    if (!(await host.onComplete(payload))) throw new Error("참가를 마치지 못했어요. 다시 시도해 주세요.");
  }

  async function selectRoom(roomId: string, pending = record.current) {
    if (secureAdmission.current) { await secureAdmission.current.select(roomId); return; }
    if (!host || !pending) return;
    const payload = await selectMemberConnect(pending, roomId, host.clientId, host.deviceToken);
    if (!active.current) return;
    if (payload.server_id !== pending.server_id || !(await host.onComplete(payload))) throw new Error("참가를 마치지 못했어요. 다시 시도해 주세요.");
  }

  async function prepare() {
    setConsent(null); setRooms([]);
    secureAdmission.current?.close(); secureAdmission.current = null;
    if (secureEntry) {
      const session = loadCentralSession();
      if (!session) { setStatus("로그인하면 참가를 이어갈 수 있어요."); return; }
      const target = await previewSecureMember(secureEntry);
      if (!active.current) return;
      setConsent({ request: { ...secureEntry, challenge_hash: "", handoff_state: "", ...(secureEntry.inviteToken ? {} : { purpose: "connect" as const }) }, target, account: session.person.display_name, sessionToken: session.token });
      setStatus(""); return;
    }
    let targetRequest = request;
    if (host) {
      setStatus("참가를 준비하고 있어요");
      const challenge = host.purpose === "connect" ? await challengeMemberConnect(host.deviceToken) : await challengeRoomMember(host.inviteToken, host.deviceToken);
      const expected = host.callback?.connect || host.callback?.record;
      if (host.purpose === "connect" && expected && (challenge.server_id !== expected.server_id || challenge.registration_epoch !== expected.registration_epoch)) throw new Error("서버 주소가 변경됐어요. 다시 시도해 주세요.");
      if (!active.current) return;
      record.current = { ...createMemberHandoff(challenge, host.inviteToken, host.meetingId), ...(host.purpose ? { purpose:host.purpose } : {}) };
      targetRequest = { ...memberTargetRequest(record.current), room_name: memberRoomName(host.roomName) };
      if (!isDesktopWebview()) {
        const central = centralAccountEntryUrl();
        if (!central) throw new Error("참가를 마치지 못했어요. 다시 시도해 주세요.");
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
    return () => { active.current = false; loginAbort.current?.abort(); secureAdmission.current?.close(); window.removeEventListener("pageshow", returned); };
    // This component owns one selected invite/return. Explicit retries use prepare().
  }, []);

  async function confirm(restoreHidden = false) {
    if (!consent || busy) return;
    await run(async () => {
      setStatus("참가를 준비하고 있어요");
      if (loadCentralSession()?.token !== consent.sessionToken) throw new Error("로그인 계정이 바뀌었어요. 다시 시도해 주세요.");
      if (record.current && record.current.expires_at <= Date.now() / 1000) throw new Error("참가 요청이 만료됐어요. 다시 시도해 주세요.");
      if (restoreHidden) await setCentralMemberHidden(consent.request, false);
      if (secureEntry && consent.target.target) {
        loginAbort.current = new AbortController();
        secureAdmission.current?.close();
        const admission = await SecureMemberAdmission.open(secureEntry, consent.target.target, consent.sessionToken, loginAbort.current.signal);
        secureAdmission.current = admission;
        try {
          const available = await admission.admit();
          if (!active.current) { admission.close(); return; }
          if (available?.length === 1) await admission.select(available[0].room_id);
          else if (available) { setConsent(null); setRooms(available); setStatus(""); }
        } catch (error) { admission.close(); throw error; }
        return;
      }
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
    if (host || secureEntry) { void run(prepare); return; }
    clearCentralMemberRequest();
    if (consent) window.location.assign(memberRetryUrl(consent.target.endpoint_origin, consent.request));
    else window.history.back();
  }

  const roomName = consent?.request.room_name;
  const reconnect = consent?.request.purpose === "connect";
  const title = reconnect ? `‘${consent.target.label}’에 다시 연결할까요?` : roomName ? `‘${roomName}’에 참가할까요?` : "이 방에 참가할까요?";

  return <GuestJoinProfilePanel displayName="" busy={busy} status={error || status}
    title={rooms.length ? "참가 중인 방을 선택해 주세요" : consent && !error ? title : "로그인하고 참가"}
    identityLabel={consent && !error ? roomName : undefined}
    serverLabel={consent && !error && !reconnect ? `${consent.target.label}에서 열린 방` : undefined}
    titleContent={consent && !error && !reconnect ? <>{roomName ? <>‘<strong>{roomName}</strong>’에 참가할까요?</> : title}</> : undefined}
    retryMode={error ? "join" : undefined} onJoin={retry}
    onDisplayNameChange={() => {}} onAvatarImageChange={() => {}}>
    <section aria-label="방 참가" className="grid gap-3 text-text-primary">
      {rooms.map(room => <button key={room.room_id} className="dc-guest-join-button" disabled={busy} onClick={() => void run(() => selectRoom(room.room_id))}>{room.name || "이름 없는 방"}</button>)}
      {consent && !error && <>
        <p className="text-sm preserve-words">{consent.account} 계정으로 참가</p>
        <p className="truncate text-xs text-text-muted">{consent.target.endpoint_origin}</p>
        <p className="text-sm text-text-muted">참가하면 이 방에 내 이름과 프로필이 보여요.</p>
        <button type="button" className="dc-guest-join-button" disabled={busy} onClick={() => void confirm()}>{reconnect ? "다시 연결" : "참가하기"}</button>
      </>}
      {consent && <button type="button" className="dc-join-cancel" disabled={busy} onClick={() => void confirm(true)}>목록에 다시 표시하고 참가</button>}
      {!host && !busy && !error && !consent && rooms.length === 0 && <button type="button" className="dc-guest-join-button"
        onClick={() => void run(async () => {
          loginAbort.current = new AbortController();
          if (secureEntry) {
            if (isDesktopWebview()) await loginCentralGoogle(() => {}, loginAbort.current.signal);
            else await loginMemberPopup(loginAbort.current.signal);
            await prepare();
          } else await startCentralWebGoogle(loginAbort.current.signal);
        })}>Google로 계속</button>}
      {(!host || onCancel) && !busy && <button type="button" className="dc-join-cancel"
        onClick={onCancel || (() => { clearCentralMemberRequest(); window.history.back(); })}>취소</button>}
    </section>
  </GuestJoinProfilePanel>;
}
