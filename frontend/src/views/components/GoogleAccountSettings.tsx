import { useCallback, useEffect, useRef, useState } from "react";
import {
  connectGoogleAccount, disconnectGoogleAccount, fetchAccountStatus, startGoogleAccountLogin,
  type AccountStatus, type GoogleChallenge,
} from "../../api/accounts";
import type { UserProfileIdentity } from "../../api/userProfile";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { clearRememberedGuestProfile } from "../../lib/deviceIdentity";
import { googleIdentityApi, loadGoogleIdentityScript } from "../../lib/googleIdentity";
import { persistRoomGuestSession } from "../../lib/roomGuestSession";

const errorMessage = (error: unknown) => error instanceof Error ? error.message : "계정 요청을 완료하지 못했어요.";

export default function GoogleAccountSettings({ identity }: { identity: UserProfileIdentity }) {
  const [status, setStatus] = useState<AccountStatus | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [challenge, setChallenge] = useState<GoogleChallenge | null>(null);
  const [credential, setCredential] = useState("");
  const [refresh, setRefresh] = useState(0);
  const mounted = useRef(false);
  const operation = useRef(false);
  const generation = useRef(0);
  const desktop = isDesktopWebview();
  const deviceToken = identity.deviceToken;
  const sessionToken = identity.sessionToken;

  useEffect(() => {
    mounted.current = true;
    return () => { mounted.current = false; };
  }, []);
  useEffect(() => {
    let active = true;
    generation.current += 1;
    operation.current = false;
    setBusy(false);
    setStatus(null);
    setChallenge(null);
    setCredential("");
    setError("");
    fetchAccountStatus({ deviceToken, sessionToken })
      .then((value) => { if (active) setStatus(value); })
      .catch((reason: unknown) => { if (active) setError(errorMessage(reason)); });
    return () => { active = false; };
  }, [deviceToken, sessionToken, refresh]);

  const acceptCredential = useCallback((value: string) => {
    setCredential((current) => current || value);
  }, []);
  const reportGoogleError = useCallback((reason: unknown) => setError(errorMessage(reason)), []);

  async function prepare() {
    if (operation.current) return;
    operation.current = true;
    const epoch = generation.current;
    setBusy(true);
    setError("");
    setChallenge(null);
    try {
      const [next] = await Promise.all([startGoogleAccountLogin(identity), loadGoogleIdentityScript()]);
      if (mounted.current && generation.current === epoch) setChallenge(next);
    } catch (reason) {
      if (mounted.current && generation.current === epoch) setError(errorMessage(reason));
    } finally {
      if (generation.current === epoch) operation.current = false;
      if (mounted.current && generation.current === epoch) setBusy(false);
    }
  }
  async function connect() {
    if (operation.current || !challenge || !credential) return;
    operation.current = true;
    const epoch = generation.current;
    setBusy(true);
    setError("");
    try {
      const result = await connectGoogleAccount(identity, credential, challenge.nonce);
      if (!mounted.current || generation.current !== epoch) return;
      if (result.identity_switched) {
        persistRoomGuestSession(null);
        clearRememberedGuestProfile();
        window.location.reload();
        return;
      }
      setStatus((current) => current ? { ...current, account: result.account } : current);
    } catch (reason) {
      if (mounted.current && generation.current === epoch) setError(errorMessage(reason));
    } finally {
      if (generation.current === epoch) operation.current = false;
      if (mounted.current && generation.current === epoch) {
        setCredential("");
        setChallenge(null);
        setBusy(false);
      }
    }
  }
  async function disconnect() {
    if (operation.current) return;
    operation.current = true;
    const epoch = generation.current;
    setBusy(true);
    setError("");
    try {
      await disconnectGoogleAccount(identity);
      if (mounted.current && generation.current === epoch) setStatus((current) => current ? { ...current, account: null } : current);
    } catch (reason) {
      if (mounted.current && generation.current === epoch) setError(errorMessage(reason));
    } finally {
      if (generation.current === epoch) operation.current = false;
      if (mounted.current && generation.current === epoch) setBusy(false);
    }
  }

  return (
    <section className="dc-guest-recovery-settings" style={{ marginTop: 24, paddingTop: 24, borderTop: "1px solid var(--color-panel-border)", minWidth: 0 }} aria-label="Google 계정 연결" aria-busy={busy}>
      <h4>Google 계정</h4>
      <p>연결한 계정으로 다른 기기에서도 이 서버의 신원을 이어갈 수 있어요.</p>
      {!status && !error && <p role="status">계정 상태를 불러오고 있어요.</p>}
      {status?.account && <>
        <strong>Google 계정이 연결됐어요.</strong>
        <p>연결을 해제해도 프로필과 방 참여는 유지돼요.</p>
        <button type="button" className="ops-button" style={{ minHeight: 44 }} disabled={busy} onClick={() => void disconnect()}>
          {busy ? "해제 중…" : "연결 해제"}
        </button>
      </>}
      {status && !status.account && desktop && <p>앱의 Google 로그인은 시작 화면의 중앙 계정에서 관리해요.</p>}
      {status && !status.account && !desktop && (!status.google.enabled
        ? <p>이 서버에는 Google 로그인이 설정되지 않았어요.</p>
        : <>
          {!challenge && <button type="button" className="ops-button is-primary" style={{ minHeight: 44 }} disabled={busy} onClick={() => void prepare()}>
            {busy ? "로그인 준비 중…" : "Google 로그인 준비"}
          </button>}
          {challenge && <GoogleButton challenge={challenge} onCredential={acceptCredential} onError={reportGoogleError} />}
        </>)}
      {error && <p role="alert" className="dc-channel-composer-error">{error}</p>}
      {!status && error && <button type="button" className="ops-button" onClick={() => setRefresh((value) => value + 1)}>다시 불러오기</button>}
      {credential && <AccountSwitchConfirmation busy={busy} onCancel={() => setCredential("")} onConnect={() => void connect()} />}
    </section>
  );
}

function GoogleButton({ challenge, onCredential, onError }: {
  challenge: GoogleChallenge;
  onCredential: (credential: string) => void;
  onError: (reason: unknown) => void;
}) {
  const targetRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const target = targetRef.current;
    const api = googleIdentityApi();
    if (!target || !api) { onError(new Error("Google 로그인 모듈을 사용할 수 없어요.")); return; }
    let active = true;
    api.initialize({ client_id: challenge.client_id, nonce: challenge.nonce, callback: (response) => {
      if (!active) return;
      if (!response.credential) { onError(new Error("Google이 로그인 응답을 반환하지 않았어요.")); return; }
      onCredential(response.credential);
    } });
    api.renderButton(target, { type: "standard", theme: "filled_black", size: "large", text: "continue_with", shape: "rectangular", width: Math.min(400, Math.floor(target.getBoundingClientRect().width)) });
    return () => { active = false; api.cancel(); target.replaceChildren(); };
  }, [challenge, onCredential, onError]);
  return <div ref={targetRef} style={{ width: "100%", minHeight: 44 }} />;
}

function AccountSwitchConfirmation({ busy, onCancel, onConnect }: { busy: boolean; onCancel: () => void; onConnect: () => void }) {
  const dialogRef = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const dialog = dialogRef.current;
    dialog?.showModal();
    return () => dialog?.close();
  }, []);
  return <div className="dc-modal-backdrop" role="presentation"><dialog ref={dialogRef} className="dc-create-channel-modal" style={{ width: "min(440px, calc(100vw - 32px))", maxHeight: "calc(100dvh - 32px)", margin: "auto", border: "1px solid var(--color-panel-border)", color: "var(--color-text-primary)", padding: 24, overflowY: "auto" }} aria-labelledby="google-account-confirm-title"
    onCancel={(event) => { event.preventDefault(); if (!busy) onCancel(); }}
    onClick={(event) => { if (event.target === event.currentTarget && !busy) onCancel(); }}>
    <section style={{ display: "grid", gap: 20 }}>
      <h3 id="google-account-confirm-title">Google 계정을 연결할까요?</h3>
      <p style={{ margin: 0, fontSize: 14, lineHeight: 1.7 }}>이미 이 서버에서 쓰던 계정이면 그 계정으로 전환돼요. 현재 게스트 프로필, 방 참여와 복구 코드는 폐기되며, 다시 참여하려면 새 초대가 필요해요. 이전 대화 기록은 남아요.</p>
      <div className="dc-create-channel-actions" style={{ gap: 12 }}>
        <button type="button" className="ops-button" style={{ minHeight: 44 }} disabled={busy} onClick={onCancel} autoFocus>취소</button>
        <button type="button" className="ops-button is-primary" style={{ minHeight: 44 }} disabled={busy} onClick={onConnect}>{busy ? "연결 중…" : "연결"}</button>
      </div>
    </section>
  </dialog></div>;
}
