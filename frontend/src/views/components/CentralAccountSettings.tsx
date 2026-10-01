import { useRef, useState } from "react";
import { centralIdentityConfigured, loadCentralSession, logoutCentral } from "../../lib/centralIdentity";

export default function CentralAccountSettings({ disabled }: { disabled: boolean }) {
  const [session] = useState(loadCentralSession);
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
      setError(reason instanceof Error ? reason.message : "로그아웃하지 못했어요. 다시 시도해 주세요.");
      operation.current = false;
      setBusy(false);
    }
  }

  if (!centralIdentityConfigured()) return null;
  return <section className="dc-guest-recovery-settings" style={{ marginTop: 24, paddingTop: 24, borderTop: "1px solid var(--color-panel-border)" }} aria-label="앱 계정" aria-busy={busy}>
    <h4>{session?.person.identity_kind === "google" ? "Google 계정" : session ? "게스트 계정" : "앱 계정"}</h4>
    {session && <p>로그인한 계정: <strong>{session.person.display_name}</strong></p>}
    <p>위의 표시 이름과 프로필 사진은 이 컴퓨터에서 쓰는 프로필이에요. 로그인 계정을 바꿔도 직접 수정한 프로필은 유지돼요.</p>
    <p>이 기기에서 로그아웃해요. 방과 프로필은 그대로 남아요.</p>
    <button type="button" className="ops-button" style={{ minHeight: 44 }} disabled={disabled || busy} onClick={() => void logout()}>
      {busy ? "로그아웃 중…" : "로그아웃"}
    </button>
    {error && <p role="alert" className="dc-channel-composer-error">{error}</p>}
  </section>;
}
