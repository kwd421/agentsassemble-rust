import { useRef, useState } from "react";
import { ApiError } from "../../lib/apiErrors";
import { providerInstallOperation } from "../../api/providerOperations";
import type { ProviderInstall } from "../../types/generated/ProviderInstall";

// Owner responses that prove no installer is still running for this request.
const TERMINAL_INSTALL_FAILURES = [
  "provider_install_unsupported", "provider_install_already_installed", "provider_install_npm_missing",
  "provider_install_offer_changed", "provider_install_unavailable", "provider_install_invalid",
  "provider_install_unconfirmed", "provider_install_outside_path", "provider_install_catalog_unavailable",
];

/**
 * Offers to install a missing provider CLI. Nothing runs until the user reads the exact
 * command and confirms it; the runtime then runs only that offered version.
 */
export default function ProviderInstallPrompt({ providerId, displayName, onUpdating, onInstalled }: {
  providerId: string;
  displayName: string;
  onUpdating?: (updating: boolean) => void;
  onInstalled?: () => void;
}) {
  const [offer, setOffer] = useState<ProviderInstall | null>(null);
  const [phase, setPhase] = useState<"idle" | "checking" | "confirming" | "installing" | "done">("idle");
  const [error, setError] = useState("");
  const inflight = useRef(false);

  async function check() {
    if (inflight.current) return;
    inflight.current = true;
    setPhase("checking");
    setError("");
    try {
      setOffer(await providerInstallOperation(providerId));
      setPhase("confirming");
    } catch (failure) {
      if (failure instanceof ApiError && failure.code === "provider_install_already_installed") onInstalled?.();
      setOffer(null);
      setPhase("idle");
      setError(failure instanceof Error ? failure.message : "설치 정보를 확인하지 못했어요.");
    } finally {
      inflight.current = false;
    }
  }

  async function install() {
    if (inflight.current || !offer) return;
    inflight.current = true;
    onUpdating?.(true);
    setPhase("installing");
    setError("");
    try {
      await providerInstallOperation(providerId, offer.version);
      setPhase("done");
      onUpdating?.(false);
      onInstalled?.();
    } catch (failure) {
      const terminal = failure instanceof ApiError && TERMINAL_INSTALL_FAILURES.includes(failure.code);
      if (terminal) onUpdating?.(false);
      if (failure instanceof ApiError && failure.code === "provider_install_catalog_unavailable") onInstalled?.();
      setOffer(null);
      setPhase("idle");
      const message = failure instanceof Error ? failure.message : "설치하지 못했어요.";
      setError(terminal ? message : `${message} 설치가 끝났는지 상태를 다시 확인해 주세요.`);
    } finally {
      inflight.current = false;
    }
  }

  return <section className="dc-agent-section" aria-label={`${displayName} CLI 설치`}>
    {phase === "done" ? <p role="status" className="dc-agent-hint preserve-words">
      {displayName} CLI {offer?.version}을 설치했어요.
    </p> : phase === "installing" ? <p role="status" className="dc-agent-hint preserve-words">
      {displayName} CLI를 설치하고 있어요. 몇 분 걸릴 수 있어요…
    </p> : phase === "confirming" && offer ? <>
      <p className="dc-agent-hint preserve-words">
        {displayName} CLI {offer.version}을 npm으로 설치할게요. 아래 명령을 이 PC에서 실행해요.
      </p>
      <pre className="dc-agent-hint" style={{ whiteSpace: "pre-wrap", overflowWrap: "anywhere" }}>{offer.command.join(" ")}</pre>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 8 }}>
        <button type="button" className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }}
          onClick={() => void install()}>설치</button>
        <button type="button" className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }}
          onClick={() => { setOffer(null); setPhase("idle"); }}>취소</button>
      </div>
    </> : <>
      <p className="dc-agent-hint preserve-words">이 앱에서 {displayName} CLI를 설치할 수 있어요. 실행할 명령을 먼저 보여드려요.</p>
      <button type="button" className="ops-button rounded-lg px-4 py-2" style={{ minHeight: 44 }}
        disabled={phase === "checking"} onClick={() => void check()}>
        {phase === "checking" ? "설치 정보 확인 중…" : "앱에서 설치하기"}
      </button>
    </>}
    {error && <p role="alert" className="dc-agent-hint preserve-words">{error}</p>}
  </section>;
}
