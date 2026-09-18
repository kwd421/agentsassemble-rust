import { useCallback, useEffect, useRef, useState } from "react";
import { ArrowUp, Check, CircleAlert, ExternalLink, LoaderCircle, RotateCw } from "lucide-react";
import { ApiError } from "../../lib/apiErrors";
import { providerUpdateOperation } from "../../api/providerOperations";
import { openProviderSetupHelp } from "../../lib/desktopBridge";
import type { ProviderAvailability } from "../../types/generated/ProviderAvailability";
import type { ProviderUpdate } from "../../types/generated/ProviderUpdate";
import ProviderSetupCard, { useTransientResult, VersionShift } from "./ProviderSetupCard";

export default function ProviderUpdatePrompt({ providerId, provider, onUpdating, onUpdated }: {
  providerId: string;
  provider?: ProviderAvailability;
  onUpdating?: (updating: boolean) => void;
  onUpdated?: () => void;
}) {
  const [observation, setObservation] = useState<ProviderUpdate | null>(null);
  const [busy, setBusy] = useState(false);
  const [request, setRequest] = useState<"check" | "update" | null>(null);
  const [startedAt, setStartedAt] = useState(0);
  const [error, setError] = useState<{ message: string; code?: string } | null>(null);
  const [deferred, setDeferred] = useState(false);
  const inflight = useRef(false);
  const updatingCallback = useRef(onUpdating);
  updatingCallback.current = onUpdating;
  const updatedCallback = useRef(onUpdated);
  updatedCallback.current = onUpdated;
  const run = useCallback(async (version?: string) => {
    if (inflight.current) return;
    inflight.current = true;
    // A read may join an installer owned by an earlier view or lost request.
    updatingCallback.current?.(true);
    setBusy(true);
    setRequest(version === undefined ? "check" : "update");
    setStartedAt(Date.now());
    setError(null);
    try {
      const result = await providerUpdateOperation(providerId, version);
      setObservation(result);
      if (result.completed) updatedCallback.current?.();
      updatingCallback.current?.(false);
    } catch (failure) {
      setObservation(null);
      if (failure instanceof ApiError && failure.code === "provider_update_catalog_unavailable") {
        updatedCallback.current?.();
      }
      // Only these owner responses confirm that no updater remains active.
      // Transport/authentication errors, busy and cleanup uncertainty retain the guard.
      const terminalFailure = failure instanceof ApiError && [
        "provider_update_unsupported", "provider_update_missing", "provider_update_offer_changed",
        "provider_update_unavailable", "provider_update_invalid",
        "provider_update_installation_unconfirmed", "provider_update_catalog_unavailable",
      ].includes(failure.code);
      if (terminalFailure) updatingCallback.current?.(false);
      const message = failure instanceof Error ? failure.message : "버전을 확인하지 못했어요.";
      setError({ message: version && !terminalFailure ? `${message} 업데이트 결과를 다시 확인해 주세요.` : message,
        code: failure instanceof ApiError ? failure.code : undefined });
    } finally {
      inflight.current = false;
      setBusy(false);
      setRequest(null);
    }
  }, [providerId]);
  useEffect(() => {
    let active = true;
    void Promise.resolve().then(() => { if (active) void run(); });
    return () => { active = false; };
  }, [run]);
  useEffect(() => {
    // A fresh owner projection can resolve the catalog failure without another updater.
    if (provider?.startable && provider.discovery_status === "ready") {
      setError((current) => current?.code === "provider_update_catalog_unavailable" ? null : current);
    }
  }, [provider]);
  async function update() {
    if (inflight.current || !observation) return;
    await run(observation.latest_version);
  }
  const completed = Boolean(observation?.completed) && !busy && !error;
  const result = useTransientResult(completed);
  const offered = !busy && !error && observation?.update_available && !observation.completed;
  // A check blocks agent creation while it may be joining an update another view started,
  // so it stays visible; an idle provider with nothing newer shows nothing.
  if (deferred || result === "gone" || (!offered && !completed && !error && !busy)) return null;
  const label = `${provider?.display_name || "제공자"} 업데이트`;

  if (completed && observation) {
    return <ProviderSetupCard label={label} tone="done" glyph={<Check size={15} strokeWidth={3} />}
      leaving={result === "leaving"}>
      <p className="dc-setup-text" role="status">{observation.current_version} 버전으로 업데이트했어요.</p>
    </ProviderSetupCard>;
  }

  if (busy) {
    const updating = request === "update";
    return <ProviderSetupCard label={label} tone={updating ? "update" : "quiet"} busy
      glyph={<LoaderCircle size={15} className="dc-setup-spin" />} since={updating ? startedAt : undefined}>
      {updating
        ? <p className="dc-setup-text" role="status"><strong>업데이트하는 중</strong>
          {observation && <VersionShift from={observation.current_version} to={observation.latest_version} />}</p>
        : <p className="dc-setup-text" role="status">새 버전을 확인하고 있어요…</p>}
    </ProviderSetupCard>;
  }

  if (error) {
    return <ProviderSetupCard label={label} tone="error" glyph={<CircleAlert size={15} strokeWidth={2.5} />}
      actions={<button type="button" className="dc-setup-button" data-variant="ghost" onClick={() => void run()}>
        <RotateCw size={13} aria-hidden="true" />버전 다시 확인</button>}>
      <p className="dc-setup-text" role="alert">{error.message}</p>
    </ProviderSetupCard>;
  }

  if (!observation) return null;
  return <ProviderSetupCard label={label} tone="update" glyph={<ArrowUp size={15} strokeWidth={2.75} />}
    actions={<>
      <button type="button" className="dc-setup-button" data-variant="ghost" onClick={() => setDeferred(true)}>나중에</button>
      {observation.native_update
        ? <button type="button" className="dc-setup-button" data-variant="primary" onClick={() => void update()}>업데이트</button>
        : <button type="button" className="dc-setup-button" data-variant="primary"
          onClick={() => void openProviderSetupHelp(providerId).catch((failure: unknown) =>
            setError({ message: failure instanceof Error ? failure.message : "공식 안내를 열지 못했어요." }))}>
          업데이트 방법 보기<ExternalLink size={13} aria-hidden="true" /></button>}
    </>}>
    <p className="dc-setup-text" role="status"><strong>새 버전</strong>
      <VersionShift from={observation.current_version} to={observation.latest_version} /></p>
  </ProviderSetupCard>;
}
