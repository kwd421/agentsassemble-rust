import { useState } from "react";
import { cancelProviderLogin, loginProvider } from "../../api/providerOperations";
import { isDesktopWebview } from "../../lib/desktopBridge";

export default function ProviderLogin({ providerId, displayName }: { providerId: string; displayName: string }) {
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [status, setStatus] = useState("");
  if (!isDesktopWebview()) return null;

  async function login() {
    setBusy(true);
    setStatus("브라우저에서 로그인을 마쳐 주세요.");
    try {
      await loginProvider(providerId);
      setStatus("로그인을 완료했어요.");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "로그인하지 못했어요.");
    } finally {
      setBusy(false);
    }
  }

  async function cancel() {
    setCancelling(true);
    try {
      if (await cancelProviderLogin(providerId)) setStatus("로그인을 취소했어요.");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "로그인 취소를 확인하지 못했어요.");
    } finally {
      setCancelling(false);
    }
  }

  return <section className="dc-agent-section" aria-label={`${displayName} 로그인`}>
    <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
      <button type="button" className="dc-agent-login-action" style={{ minHeight: 44 }} disabled={busy} onClick={() => void login()}>
        {busy ? "로그인 중…" : `${displayName} 로그인`}
      </button>
      {busy && <button type="button" className="ops-button" style={{ minHeight: 44, padding: "0 12px" }} disabled={cancelling} onClick={() => void cancel()}>
        {cancelling ? "취소 중…" : "로그인 취소"}
      </button>}
    </div>
    {status && <p role="status" className="dc-agent-hint preserve-words">{status}</p>}
  </section>;
}
