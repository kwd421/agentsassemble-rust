import { useRef, useState } from "react";
import { providerUpdateOperation } from "../../api/providerOperations";
import { openProviderSetupHelp } from "../../lib/desktopBridge";
import { ApiError } from "../../lib/apiErrors";
import type { ProviderUpdate } from "../../types/generated/ProviderUpdate";

export default function ProviderUpdatePrompt({ providerId }: { providerId: string }) {
  const [observation, setObservation] = useState<ProviderUpdate | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState("");
  const [deferred, setDeferred] = useState(false);
  const inflight = useRef(false);
  async function run(update: boolean) {
    if (inflight.current) return;
    inflight.current = true;
    setBusy(true);
    setStatus("");
    setDeferred(false);
    try {
      if (update && observation && !observation.native_update) {
        await openProviderSetupHelp(providerId);
        setStatus("공식 업데이트 안내를 열었어요. 설치 후 버전을 다시 확인해 주세요.");
      } else {
        const result = await providerUpdateOperation(providerId, update ? observation?.latest_version : undefined);
        setObservation(result);
        if (result.handoff_started) setStatus("업데이트 터미널을 열었어요. 터미널에서 결과를 확인한 뒤 버전을 다시 확인해 주세요.");
      }
    } catch (error) {
      setObservation(null);
      const message = error instanceof Error ? error.message : "버전을 확인하지 못했어요.";
      setStatus(update && observation?.native_update && !(error instanceof ApiError)
        ? `${message} 업데이트 터미널이 열렸을 수 있으니 확인한 뒤 다시 시도해 주세요.` : message);
    } finally {
      inflight.current = false;
      setBusy(false);
    }
  }
  const offered = observation?.update_available && !observation.handoff_started && !deferred;
  return <section className="dc-agent-section" aria-label="제공자 버전">
    <button type="button" className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }}
      disabled={busy} onClick={() => void run(false)}>{busy ? "진행 중…" : "새 버전 확인"}</button>
    {observation && <p className="dc-agent-hint preserve-words" role="status">
      현재 {observation.current_version} · 제공 버전 {observation.latest_version}
      {!observation.update_available && " · 현재 버전보다 새로운 버전이 없어요."}
    </p>}
    {offered && <>
      <p className="dc-agent-hint preserve-words">새 버전이 있어요. 지금 업데이트할까요?</p>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
        <button type="button" className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }} disabled={busy}
          onClick={() => void run(true)}>{observation.native_update ? "업데이트" : "업데이트 방법 보기"}</button>
        <button type="button" className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }} disabled={busy}
          onClick={() => { setDeferred(true); setStatus("나중에 업데이트할게요. 현재 버전으로 계속 사용할 수 있어요."); }}>나중에</button>
      </div>
    </>}
    {status && <p role="status" className="dc-agent-hint preserve-words">{status}</p>}
  </section>;
}
