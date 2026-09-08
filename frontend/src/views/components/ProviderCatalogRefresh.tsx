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
      await refreshProviderCatalog();
      setStatus("카탈로그를 갱신했어요.");
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "카탈로그를 갱신하지 못했어요.");
    } finally {
      setBusy(false);
    }
  }

  return <div className="dc-agent-section">
    <button type="button" className="ops-button" style={{ minHeight: 44, padding: "0 12px", display: "inline-flex", alignItems: "center", gap: 8 }} disabled={busy} onClick={() => void refresh()}>
      <RefreshCw size={16} aria-hidden="true" /> {busy ? "갱신 중…" : "카탈로그 갱신"}
    </button>
    {status && <p role="status" className="dc-agent-hint preserve-words">{status}</p>}
  </div>;
}
