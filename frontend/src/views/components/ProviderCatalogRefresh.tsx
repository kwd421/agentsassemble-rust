import { useState } from "react";
import { RefreshCw } from "lucide-react";
import { refreshProviderCatalog } from "../../api/providerOperations";
import { isDesktopWebview } from "../../lib/desktopBridge";

export default function ProviderCatalogRefresh() {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  if (!isDesktopWebview()) return null;

  async function refresh() {
    setBusy(true);
    setStatus("");
    try {
      const catalog = await refreshProviderCatalog();
      const confirmed = catalog.providers.filter((provider) => provider.discovery_status === "ready").length;
      const unavailable = catalog.providers.length - confirmed;
      setStatus(unavailable
        ? `모델 목록을 새로고침했어요. ${confirmed}개 제공자 확인, ${unavailable}개 확인 불가. 각 제공자의 안내를 확인해 주세요.`
        : "모델 목록을 새로고침했어요.");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "모델 목록을 새로고침하지 못했어요.");
    } finally {
      setBusy(false);
    }
  }

  return <div className="dc-agent-section">
    <button type="button" className="ops-button" style={{ minHeight: 44, padding: "0 12px", display: "inline-flex", alignItems: "center", gap: 8 }} disabled={busy} onClick={() => void refresh()}>
      <RefreshCw size={16} aria-hidden="true" /> {busy ? "새로고침 중…" : "모델 목록 새로고침"}
    </button>
    <p className="dc-agent-hint preserve-words">제공자의 최신 목록을 다시 확인해요. 사용 중인 에이전트의 모델은 바뀌지 않아요.</p>
    {status && <p role="status" className="dc-agent-hint preserve-words">{status}</p>}
  </div>;
}
