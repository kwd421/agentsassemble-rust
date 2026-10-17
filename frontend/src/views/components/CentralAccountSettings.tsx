import ServerBackupCard from "./ServerBackupCard";
import AccountDeletionSettings from "./AccountDeletionSettings";
import { useEffect, useRef, useState } from "react";
import { CENTRAL_SESSION_CLEARED_EVENT, CENTRAL_SESSION_CHANGED_EVENT, centralAccountEntryUrl, centralIdentityConfigured, loadCentralSession, logoutCentral } from "../../lib/central/identity";

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
      if (reason instanceof TypeError || (typeof DOMException !== "undefined" && reason instanceof DOMException)) setError("로그아웃하지 못했어요. 다시 시도해 주세요.");
      else {
        const text = typeof reason === "string" ? reason : reason instanceof Error ? reason.message : "";
        const line = text.split("\n")[0]?.trim() ?? "";
        setError(line && /[가-힣]/.test(line) ? line : "로그아웃하지 못했어요. 다시 시도해 주세요.");
      }
      operation.current = false;
      setBusy(false);
    }
  }

  if (!centralIdentityConfigured()) return null;
  if (!session) return <section className="dc-guest-recovery-settings" aria-label="앱 계정">
    <h4>앱 계정</h4>
    <p>로그인하면 계정과 내 서버를 확인할 수 있어요.</p>
    {showEntryLink && <a className="min-h-11 w-fit text-[13px] text-text-muted hover:underline" href={`${centralAccountEntryUrl()}?account=settings`} target="_blank" rel="noopener noreferrer">계정 설정</a>}
    <AccountDeletionSettings disabled={disabled || busy} />
  </section>;
  return <section className="dc-guest-recovery-settings" style={{ marginTop: 24, paddingTop: 24, borderTop: "1px solid var(--color-panel-border)" }} aria-label="앱 계정" aria-busy={busy}>
    <h4>{session?.person.identity_kind === "google" ? "Google 계정" : session ? "게스트 계정" : "앱 계정"}</h4>
    {session && <p>로그인한 계정: <strong>{session.person.display_name}</strong></p>}
    <button type="button" className="ops-button" style={{ minHeight: 44, width: "fit-content", fontSize: 13, padding: "0 16px" }} disabled={disabled || busy} onClick={() => void logout()}>
      {busy ? "로그아웃 중…" : "로그아웃"}
    </button>
    <ServerBackupCard disabled={disabled || busy} />
    <AccountDeletionSettings disabled={disabled || busy} />
    {error && <p role="alert" className="dc-channel-composer-error">{error}</p>}
  </section>;
}
