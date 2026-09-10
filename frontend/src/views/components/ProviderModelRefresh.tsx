import { useCallback, useEffect, useRef, useState } from "react";
import { RefreshCw } from "lucide-react";
import { refreshLocalProviderCatalog } from "../../api/providerOperations";
import type { ProviderCatalog } from "../../types/generated/ProviderCatalog";
import { isDesktopWebview } from "../../lib/desktopBridge";

export default function ProviderModelRefresh({ title, providerId, automaticAllowed, localAvailable = true, onCatalogChange }: {
  title: string; providerId: string; automaticAllowed: boolean; localAvailable?: boolean; onCatalogChange?: (catalog: ProviderCatalog) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const desktop = localAvailable && isDesktopWebview();
  const generation = useRef(0);
  const catalogChanged = useRef(onCatalogChange);
  catalogChanged.current = onCatalogChange;

  const refresh = useCallback(async (force: boolean, signal?: AbortSignal) => {
    if (!providerId) return;
    const current = ++generation.current;
    setBusy(true);
    setStatus("");
    try {
      const catalog = await refreshLocalProviderCatalog(providerId, force, signal);
      if (signal?.aborted || current !== generation.current) return;
      catalogChanged.current?.(catalog);
      const provider = catalog.providers.find((entry) => entry.id === providerId);
      if (!provider || provider.discovery_status !== "ready") {
        throw new Error(provider?.discovery_error || "이 제공자의 모델 목록을 확인하지 못했어요.");
      }
      setStatus(force ? "모델 목록을 새로고침했어요." : "");
    } catch (error) {
      if (!signal?.aborted && current === generation.current) {
        setStatus(error instanceof Error ? error.message : "모델 목록을 새로고침하지 못했어요.");
      }
    } finally {
      if (!signal?.aborted && current === generation.current) setBusy(false);
    }
  }, [providerId]);

  useEffect(() => {
    const controller = new AbortController();
    setStatus("");
    setBusy(false);
    if (desktop && automaticAllowed && providerId) void refresh(false, controller.signal);
    return () => { generation.current += 1; controller.abort(); };
  }, [automaticAllowed, desktop, providerId, refresh]);

  return <div className="dc-agent-section">
    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
      <p className="dc-agent-section-title">{title}</p>
      {desktop && <button
        type="button"
        className="ops-button"
        aria-label="모델 목록 새로고침"
        title="모델 목록 새로고침"
        aria-busy={busy}
        style={{ width: 44, height: 44, flex: "0 0 auto", display: "grid", placeItems: "center" }}
        disabled={busy || !providerId || !automaticAllowed}
        onClick={() => void refresh(true)}
      >
        <RefreshCw size={16} aria-hidden="true" />
      </button>}
    </div>
    {(busy || status) && <p role="status" className="dc-agent-create-status preserve-words">
      {busy ? "모델 목록을 확인하고 있어요." : status}
    </p>}
  </div>;
}
