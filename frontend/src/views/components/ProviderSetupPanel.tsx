import { useCallback, useEffect, useRef, useState } from "react";
import { refreshLocalProviderCatalog } from "../../api/providerOperations";
import { requestDesktopHostProductSurface } from "../../lib/desktopBridge";
import { providerSetupDestination } from "../../lib/providerSetup";
import type { ProviderAvailability } from "../../types/generated/ProviderAvailability";
import ProviderLogin from "./ProviderLogin";
import ProviderSetupActions from "./ProviderSetupActions";

export default function ProviderSetupPanel({ providerId }: { providerId: string }) {
  const destination = providerSetupDestination(providerId);
  const [provider, setProvider] = useState<ProviderAvailability | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const readGeneration = useRef(0);
  const read = useCallback(async (refresh: boolean, signal?: AbortSignal) => {
    const generation = ++readGeneration.current;
    setBusy(true);
    setError("");
    try {
      await requestDesktopHostProductSurface();
      const catalog = await refreshLocalProviderCatalog(providerId, refresh, signal);
      if (signal?.aborted || generation !== readGeneration.current) return;
      const current = catalog.providers.find((item) => item.id === providerId);
      if (!current) throw new Error("이 앱에서 해당 제공자를 사용할 수 없어요.");
      setProvider(current);
    } catch (failure) {
      if (!signal?.aborted && generation === readGeneration.current) setError(failure instanceof Error ? failure.message : "상태를 확인하지 못했어요.");
    } finally {
      if (!signal?.aborted && generation === readGeneration.current) setBusy(false);
    }
  }, [providerId]);
  useEffect(() => {
    const controller = new AbortController();
    void read(false, controller.signal);
    return () => { readGeneration.current += 1; controller.abort(); };
  }, [read]);
  if (!destination) return <p role="alert">지원하지 않는 제공자 설정이에요.</p>;
  return <main style={{ padding: 24, maxWidth: 560, margin: "0 auto", display: "grid", gap: 20 }}>
    <header>
      <h1 className="text-2xl font-black text-text-primary">{destination.display_name} 설정</h1>
      <p className="dc-agent-hint preserve-words">이 PC에 설치된 제공자의 로그인과 설치·업데이트를 관리해요.</p>
    </header>
    <section aria-label="이 PC의 제공자 상태">
      <p role="status">{busy ? "상태를 확인하고 있어요." : provider?.startable ? "이 PC에서 사용할 준비가 됐어요."
        : provider?.discovery_error_code === "authentication_required" ? "로그인이 필요해요."
        : provider?.discovery_error_code === "command_missing" ? "이 PC에 제공자 CLI를 설치해 주세요."
        : provider?.discovery_status === "loading" ? "제공자 정보를 확인 중이에요. 잠시 후 다시 확인해 주세요."
        : provider?.discovery_error || "아직 실행 가능한 상태를 확인하지 못했어요."}</p>
      <button type="button" className="ops-button rounded-lg px-4 py-2" disabled={busy}
        style={{ minHeight: 44, marginTop: 12 }} onClick={() => void read(true)}>
        {busy ? "확인 중…" : "설정 후 상태 다시 확인"}
      </button>
      {error && <p role="alert" className="dc-agent-hint preserve-words">{error}</p>}
    </section>
    {provider?.login_supported && <ProviderLogin key={provider.id} providerId={provider.id} displayName={provider.display_name}
      onAuthenticated={() => void read(false)} />}
    <ProviderSetupActions providerId={providerId} provider={provider} />
    <p className="dc-agent-hint preserve-words">작업을 마치면 브라우저의 에이전트 추가 화면으로 돌아가 주세요.
      다른 PC에서 실행되는 에이전트의 상태는 여기서 변경되지 않아요.</p>
  </main>;
}
