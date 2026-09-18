import { useRef, useState } from "react";
import { Check, Copy, Download, ExternalLink } from "lucide-react";
import { ApiError } from "../../lib/apiErrors";
import { providerInstallOperation } from "../../api/providerOperations";
import { openProviderSetupHelp } from "../../lib/desktopBridge";
import type { ProviderInstall } from "../../types/generated/ProviderInstall";
import ProviderSetupCard, { useTransientResult } from "./ProviderSetupCard";

// Owner responses that prove no installer is still running for this request.
const TERMINAL_INSTALL_FAILURES = [
  "provider_install_unsupported", "provider_install_already_installed", "provider_install_npm_missing",
  "provider_install_offer_changed", "provider_install_unavailable", "provider_install_invalid",
  "provider_install_unconfirmed", "provider_install_outside_path", "provider_install_catalog_unavailable",
];

type Phase = "idle" | "checking" | "confirming" | "installing" | "done";

/**
 * Offers to install a missing provider CLI. Nothing runs until the user reads the exact
 * command and confirms it; the runtime then installs only that offered version.
 */
export default function ProviderInstallPrompt({ providerId, displayName, installable, onUpdating, onInstalled }: {
  providerId: string;
  displayName: string;
  /** Whether this provider publishes a CLI the runtime can install. */
  installable: boolean;
  onUpdating?: (updating: boolean) => void;
  onInstalled?: () => void;
}) {
  const [offer, setOffer] = useState<ProviderInstall | null>(null);
  const [phase, setPhase] = useState<Phase>("idle");
  const [error, setError] = useState("");
  const [copied, setCopied] = useState(false);
  const inflight = useRef(false);
  const result = useTransientResult(phase === "done");
  const heading = `provider-install-${providerId}`;

  async function check() {
    if (inflight.current) return;
    inflight.current = true;
    setPhase("checking");
    setError("");
    try {
      setOffer(await providerInstallOperation(providerId));
      setCopied(false);
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

  async function openHelp() {
    try {
      await openProviderSetupHelp(providerId);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "공식 안내를 열지 못했어요.");
    }
  }

  async function copyCommand() {
    if (!offer) return;
    try {
      await navigator.clipboard.writeText(offer.command.join(" "));
      setCopied(true);
    } catch {
      setError("명령을 복사하지 못했어요. 직접 선택해 복사해 주세요.");
    }
  }

  if (result === "gone") return null;
  const title = `${displayName} CLI`;

  if (phase === "done") {
    return <ProviderSetupCard providerId={providerId} headingId={heading} title={title} tone="done"
      badge="설치 완료" leaving={result === "leaving"}>
      <p className="dc-provider-setup-text" role="status">{title} {offer?.version}을 설치했어요.</p>
    </ProviderSetupCard>;
  }

  const installing = phase === "installing";
  return <ProviderSetupCard providerId={providerId} headingId={heading} title={title}
    tone={error ? "error" : "install"} busy={installing || phase === "checking"}
    badge={installing ? "설치 중" : phase === "checking" ? "확인 중" : error ? "확인 필요" : "설치 필요"}
    detail={phase === "confirming" && offer ? <div className="dc-provider-setup-terminal">
      <div className="dc-provider-setup-terminal-bar">
        <span>이 PC에서 실행할 명령</span>
        <button type="button" className="dc-provider-setup-copy-command" onClick={() => void copyCommand()}>
          {copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}
          {copied ? "복사됨" : "복사"}
        </button>
      </div>
      <pre className="dc-provider-setup-command" aria-label="이 PC에서 실행할 명령">{offer.command.join(" ")}</pre>
      <p className="dc-provider-setup-note">{offer.package} {offer.version}을 이 계정 권한으로 설치해요. 관리자 권한은 쓰지 않아요.</p>
    </div> : undefined}
    actions={phase === "confirming" ? <>
      <button type="button" className="dc-provider-setup-button" data-variant="ghost"
        onClick={() => { setOffer(null); setPhase("idle"); }}>취소</button>
      <button type="button" className="dc-provider-setup-button" data-variant="primary" autoFocus
        onClick={() => void install()}><Download size={15} aria-hidden="true" />설치</button>
    </> : installing ? undefined : installable ? <>
      <button type="button" className="dc-provider-setup-button" data-variant="ghost"
        onClick={() => void openHelp()}>직접 설치 방법</button>
      <button type="button" className="dc-provider-setup-button" data-variant="primary"
        disabled={phase === "checking"} onClick={() => void check()}>
        <Download size={15} aria-hidden="true" />{phase === "checking" ? "확인 중…" : "앱에서 설치하기"}
      </button>
    </> : <button type="button" className="dc-provider-setup-button" data-variant="primary"
      onClick={() => void openHelp()}><ExternalLink size={15} aria-hidden="true" />설치 안내 열기</button>}>
    <p className="dc-provider-setup-text" role="status">
      {installing ? "npm으로 설치하고 있어요. 보통 1분 안에 끝나요."
        : phase === "confirming" ? "아래 명령을 확인한 뒤 설치해 주세요."
          : installable ? "이 PC에서 찾지 못했어요. 앱에서 바로 설치할 수 있어요."
            : "이 PC에서 찾지 못했어요. 공식 안내를 따라 설치한 뒤 상태를 다시 확인해 주세요."}
    </p>
    {error && <p className="dc-provider-setup-error" role="alert">{error}</p>}
  </ProviderSetupCard>;
}
