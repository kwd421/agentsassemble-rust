import { useCallback, useEffect, useRef, useState } from "react";
import { cancelProviderLogin, loginProvider } from "../../api/providerOperations";
import { isDesktopWebview } from "../../lib/desktopBridge";
import { ApiError, userFacingError } from "../../lib/apiErrors";

const LOGIN_ERRORS: Record<string, string> = {
  provider_login_unsupported: "이 AI는 앱에서 로그인을 지원하지 않아요.",
  provider_login_missing: "AI CLI를 설치한 뒤 로그인해 주세요.",
  provider_login_timeout: "로그인 시간이 초과됐어요. 다시 시도해 주세요.",
  provider_login_cancelled: "로그인을 취소했어요.",
  provider_login_failed: "로그인하지 못했어요. 로그인 창을 확인한 뒤 다시 시도해 주세요.",
  provider_login_handoff_unconfirmed: "터미널이 열렸을 수 있어요. 확인한 뒤 카탈로그를 갱신해 주세요.",
  provider_login_cleanup_unconfirmed: "로그인 프로세스가 종료됐는지 확인하지 못했어요.",
};

function errorMessage(error: unknown, message: string): string {
  if (error instanceof ApiError && LOGIN_ERRORS[error.code]) return LOGIN_ERRORS[error.code];
  return userFacingError(error, message);
}

export default function ProviderLogin({ providerId, displayName, onAuthenticated, automatic = false }: {
  providerId: string; displayName: string; onAuthenticated?: () => void; automatic?: boolean;
}) {
  const [busy, setBusy] = useState(false);
  const [cancelling, setCancelling] = useState(false);
  const [status, setStatus] = useState("");
  const inflight = useRef(false);
  const authenticated = useRef(onAuthenticated);
  authenticated.current = onAuthenticated;
  const currentProvider = useRef(providerId);
  currentProvider.current = providerId;
  const desktop = isDesktopWebview();

  const login = useCallback(async () => {
    if (inflight.current) return;
    inflight.current = true;
    const onComplete = authenticated.current;
    setBusy(true);
    setStatus("로그인 창에서 안내를 따라 주세요.");
    try {
      const outcome = await loginProvider(providerId);
      // Authenticated needs no box (the catalog refreshes); the terminal
      // handoff still needs its instruction.
      setStatus(outcome === "started" ? "터미널에서 로그인을 마친 뒤 카탈로그를 갱신해 주세요." : "");
      if (outcome === "authenticated" && currentProvider.current === providerId) onComplete?.();
    } catch (error) {
      // A cancelled login returns to the idle button with no box.
      if (error instanceof ApiError && error.code === "provider_login_cancelled") setStatus("");
      else setStatus(errorMessage(error, "로그인하지 못했어요."));
    } finally {
      inflight.current = false;
      setBusy(false);
    }
  }, [providerId]);

  useEffect(() => {
    let active = true;
    if (desktop && automatic) {
      void Promise.resolve().then(() => { if (active) void login(); });
    }
    return () => { active = false; };
  }, [automatic, desktop, login]);

  if (!desktop) return null;

  async function cancel() {
    setCancelling(true);
    try {
      // The idle login button is the only feedback; no success box.
      if (await cancelProviderLogin(providerId)) setStatus("");
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
      {busy && <button type="button" className="dc-button dc-button--secondary dc-button--lg" style={{ minHeight: 44, padding: "0 12px" }} disabled={cancelling} onClick={() => void cancel()}>
        {cancelling ? "취소 중…" : "로그인 취소"}
      </button>}
    </div>
    {status && <p role="status" className="dc-agent-hint preserve-words">{status}</p>}
  </section>;
}
