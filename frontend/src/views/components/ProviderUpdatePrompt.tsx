import { useCallback, useEffect, useRef, useState } from "react";
import { providerUpdateOperation } from "../../api/providerOperations";
import { openProviderSetupHelp } from "../../lib/desktopBridge";
import type { ProviderUpdate } from "../../types/generated/ProviderUpdate";

export default function ProviderUpdatePrompt({ providerId, onUpdating }: {
  providerId: string;
  onUpdating?: (updating: boolean) => void;
}) {
  const [observation, setObservation] = useState<ProviderUpdate | null>(null);
  const [busy, setBusy] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState("");
  const [deferred, setDeferred] = useState(false);
  const inflight = useRef(false);
  const run = useCallback(async (version?: string) => {
    if (inflight.current) return;
    inflight.current = true;
    setBusy(true);
    setInstalling(version !== undefined);
    setError("");
    try {
      const result = await providerUpdateOperation(providerId, version);
      setObservation(result);
    } catch (failure) {
      setObservation(null);
      const message = failure instanceof Error ? failure.message : "버전을 확인하지 못했어요.";
      setError(version ? `${message} 업데이트 결과를 다시 확인해 주세요.` : message);
    } finally {
      inflight.current = false;
      setBusy(false);
    }
  }, [providerId]);
  useEffect(() => {
    let active = true;
    void Promise.resolve().then(() => { if (active) void run(); });
    return () => { active = false; };
  }, [run]);
  async function update() {
    if (inflight.current || !observation) return;
    onUpdating?.(true);
    try { await run(observation.latest_version); }
    finally { onUpdating?.(false); }
  }
  if (deferred || (!busy && !error && !observation?.update_available && !observation?.completed)) return null;
  return <section className="dc-agent-section" aria-label="제공자 버전">
    {busy ? <p role="status" className="dc-agent-hint preserve-words">{installing ? "업데이트하고 있어요…" : "새 버전을 확인하고 있어요…"}</p>
      : observation?.completed ? <p role="status" className="dc-agent-hint preserve-words">
        {observation.current_version} 버전으로 업데이트했어요.
      </p> : observation?.update_available && <>
        <p className="dc-agent-hint preserve-words">새 버전 {observation.latest_version}이 있어요. 지금 업데이트할까요?</p>
        <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
          <button type="button" className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }}
            onClick={() => {
              if (observation.native_update) void update();
              else void openProviderSetupHelp(providerId).catch((failure: unknown) =>
                setError(failure instanceof Error ? failure.message : "공식 안내를 열지 못했어요."));
            }}>{observation.native_update ? "업데이트" : "업데이트 방법 보기"}</button>
          <button type="button" className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }}
            onClick={() => setDeferred(true)}>나중에</button>
        </div>
      </>}
    {error && <>
      <p role="alert" className="dc-agent-hint preserve-words">{error}</p>
      <button type="button" className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }}
        disabled={busy} onClick={() => void run()}>버전 다시 확인</button>
    </>}
  </section>;
}
