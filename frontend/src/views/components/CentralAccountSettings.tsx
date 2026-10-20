import ServerBackupCard from "./ServerBackupCard";
import AccountDeletionSettings from "./AccountDeletionSettings";
import ParticipantAvatarContent from "./ParticipantAvatarContent";
import { loadDeletionReceipt } from "../../lib/central/accountDeletion";
import { useEffect, useRef, useState } from "react";
import { CENTRAL_SESSION_CLEARED_EVENT, CENTRAL_SESSION_CHANGED_EVENT, centralAccountEntryUrl, centralIdentityConfigured, loadCentralSession, logoutCentral } from "../../lib/central/identity";
import { userFacingError } from "../../lib/apiErrors";

export default function CentralAccountSettings({ disabled, showEntryLink = true }: { disabled: boolean; showEntryLink?: boolean }) {
  const [session, setSession] = useState(loadCentralSession);
  useEffect(() => { const changed = () => setSession(loadCentralSession()); window.addEventListener(CENTRAL_SESSION_CHANGED_EVENT, changed); window.addEventListener(CENTRAL_SESSION_CLEARED_EVENT, changed); window.addEventListener("storage", changed); return () => { window.removeEventListener(CENTRAL_SESSION_CHANGED_EVENT, changed); window.removeEventListener(CENTRAL_SESSION_CLEARED_EVENT, changed); window.removeEventListener("storage", changed); }; }, []);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const operation = useRef(false);

  async function logout() {
    if (disabled || operation.current) return;
    operation.current = true;
    setBusy(true);
    setError("");
    try {
      await logoutCentral();
      window.location.reload();
    } catch (reason) {
      setError(userFacingError(reason, "로그아웃하지 못했어요. 다시 시도해 주세요."));
      operation.current = false;
      setBusy(false);
    }
  }

  if (!centralIdentityConfigured()) return null;
  // Signed out: only a kept deletion receipt has anything to show (its progress).
  const retainedReceipt = (() => { try { return Boolean(loadDeletionReceipt()); } catch { return true; } })();
  if (!session) return <section className="dc-guest-recovery-settings dc-account-block" aria-label="앱 계정">
    <h4>앱 계정</h4>
    <p className="dc-account-block-lead">로그인하면 계정과 내 서버를 확인할 수 있어요.</p>
    {showEntryLink && <a className="dc-button dc-button--secondary dc-button--lg dc-account-block-action" href={`${centralAccountEntryUrl()}?account=settings`} target="_blank" rel="noopener noreferrer">계정 설정 열기</a>}
    {retainedReceipt && <AccountDeletionSettings disabled={disabled} label="탈퇴 진행 상황" />}
  </section>;
  const person = session.person;
  return <>
    <section className="dc-guest-recovery-settings dc-account-block" aria-label="앱 계정" aria-busy={busy}>
      <h4>{person.identity_kind === "google" ? "Google 계정" : "게스트 계정"}</h4>
      <div className="dc-account-card">
        <span className="dc-member-avatar" aria-hidden>{person.avatar_url ? <img src={person.avatar_url} alt="" referrerPolicy="no-referrer" /> : <ParticipantAvatarContent participantType="human" displayName={person.display_name} size={40} />}</span>
        <span className="dc-account-card-text">
          <strong className="preserve-words">{person.display_name}</strong>
          <small>{person.identity_kind === "google" ? "Google로 로그인했어요" : "게스트로 로그인했어요"}</small>
        </span>
        <button type="button" className="dc-button dc-button--secondary dc-button--lg dc-account-block-action" disabled={disabled || busy} onClick={() => void logout()}>
          {busy ? "로그아웃 중…" : "로그아웃"}
        </button>
      </div>
      {error && <p role="alert" className="dc-channel-composer-error">{error}</p>}
      <ServerBackupCard disabled={disabled || busy} />
    </section>
    <section className="dc-guest-recovery-settings dc-account-block" aria-label="계정 탈퇴">
      <h4>계정 탈퇴</h4>
      <p className="dc-account-block-lead">모든 기기에서 로그아웃되고, 이 계정으로 다시 들어올 수 없으며 되돌릴 수도 없어요.</p>
      <AccountDeletionSettings disabled={disabled || busy} />
    </section>
  </>;
}
