import { useState } from "react";
import { cancelProviderLogin, loginProvider } from "../../api/providerOperations";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { ApiError } from "../../lib/apiErrors";

const LOGIN_ERRORS: Record<string, string> = {
  provider_login_unsupported: "이 제공자는 앱에서 로그인을 지원하지 않아요.",
  provider_login_missing: "제공자 CLI를 설치한 뒤 로그인해 주세요.",
  provider_login_timeout: "로그인 시간이 초과됐어요. 다시 시도해 주세요.",
  provider_login_cancelled: "로그인을 취소했어요.",
  provider_login_failed: "로그인하지 못했어요. 로그인 창을 확인한 뒤 다시 시도해 주세요.",
  provider_login_handoff_unconfirmed: "터미널이 열렸을 수 있어요. 확인한 뒤 카탈로그를 갱신해 주세요.",
  provider_login_cleanup_unconfirmed: "로그인 프로세스가 종료됐는지 확인하지 못했어요.",
};

function errorMessage(error: unknown, message: string): string {
  if (error instanceof ApiError && LOGIN_ERRORS[error.code]) return LOGIN_ERRORS[error.code];
  return error instanceof Error ? error.message : message;
}

export default function ProviderLogin({ providerId, displayName }: { providerId: string; displayName: string }) {
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [status, setStatus] = useState("");
  if (!isDesktopWebview()) return null;

  async function login() {
    setBusy(true);
    setStatus("로그인 창에서 안내를 따라 주세요.");
    try {
      const outcome = await loginProvider(providerId);
      setStatus(outcome === "started" ? "터미널에서 로그인을 마친 뒤 카탈로그를 갱신해 주세요." : "로그인을 완료했어요.");
    } catch (error) {
      setStatus(errorMessage(error, "로그인하지 못했어요."));
    } finally {
      setBusy(false);
    }
  }

  async function cancel() {
    setCancelling(true);
    try {
      if (await cancelProviderLogin(providerId)) setStatus("로그인을 취소했어요.");
    } catch (error) {
      setStatus(errorMessage(error, "로그인 취소를 확인하지 못했어요."));
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
