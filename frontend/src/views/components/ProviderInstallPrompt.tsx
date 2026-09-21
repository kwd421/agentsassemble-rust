import { useRef, useState } from "react";
import { Check, CircleAlert, Copy, Download, ExternalLink, LoaderCircle } from "lucide-react";
import { ApiError } from "../../lib/apiErrors";
import { providerInstallOperation } from "../../api/providerOperations";
import { openProviderSetupHelp } from "../../lib/desktopBridge";
import { shellCommandText } from "../../lib/shellCommandText";
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
  const [startedAt, setStartedAt] = useState(0);
  const inflight = useRef(false);
  const result = useTransientResult(phase === "done");

  function completed(installation: ProviderInstall) {
    setOffer(installation);
    setPhase("done");
    onUpdating?.(false);
    onInstalled?.();
  }

  async function check() {
    if (inflight.current) return;
    inflight.current = true;
    setPhase("checking");
    setError("");
    try {
      const installation = await providerInstallOperation(providerId);
      if (installation.completed) completed(installation);
      else {
        setOffer(installation);
        setCopied(false);
        setPhase("confirming");
      }
    } catch (failure) {
      if (failure instanceof ApiError && failure.code === "provider_install_already_installed") {
        onUpdating?.(false);
        onInstalled?.();
      }
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
    setStartedAt(Date.now());
    setPhase("installing");
    setError("");
    try {
      completed(await providerInstallOperation(providerId, offer));
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
      await navigator.clipboard.writeText(shellCommandText(offer.command));
      setCopied(true);
    } catch {
      setError("명령을 복사하지 못했어요. 직접 선택해 복사해 주세요.");
    }
  }

  if (result === "gone") return null;
  const title = `${displayName} CLI`;
  const label = `${title} 설치`;

  if (phase === "done") {
    return <ProviderSetupCard label={label} tone="done" glyph={<Check size={15} strokeWidth={3} />}
      leaving={result === "leaving"}>
      <p className="dc-setup-text" role="status">{title} {offer?.version}을 설치했어요.</p>
    </ProviderSetupCard>;
  }

  const installing = phase === "installing";
  const command = offer && (phase === "confirming" || installing) ? <div className="dc-setup-term"
    data-running={installing ? "true" : undefined}>
    <pre className="dc-setup-command" aria-label="이 PC에서 실행할 명령">{shellCommandText(offer.command)}</pre>
    {!installing && <button type="button" className="dc-setup-copy" onClick={() => void copyCommand()}
      aria-label={copied ? "명령 복사됨" : "명령 복사"}>
      {copied ? <Check size={13} aria-hidden="true" /> : <Copy size={13} aria-hidden="true" />}
    </button>}
  </div> : undefined;

  if (installing && offer) {
    return <ProviderSetupCard label={label} tone="install" busy since={startedAt}
      glyph={<LoaderCircle size={15} className="dc-setup-spin" />} detail={command}>
      <p className="dc-setup-text" role="status"><strong>설치하는 중</strong>
        <span className="dc-setup-muted">보통 1분 안에 끝나요</span></p>
    </ProviderSetupCard>;
  }

  if (phase === "confirming" && offer) {
    return <ProviderSetupCard label={label} tone="install" glyph={<Download size={15} strokeWidth={2.5} />}
      detail={<>{command}<p className="dc-setup-note">
        {offer.package} {offer.version} · 이 계정 권한으로 설치해요 · 관리자 권한은 쓰지 않아요</p></>}
      actions={<>
        <button type="button" className="dc-setup-button" data-variant="ghost"
          onClick={() => { setOffer(null); setPhase("idle"); }}>취소</button>
        <button type="button" className="dc-setup-button" data-variant="primary" autoFocus
          onClick={() => void install()}>설치</button>
      </>}>
      <p className="dc-setup-text" role="status"><strong>아래 명령을 실행해요</strong>
        <span className="dc-setup-muted">확인하면 이 PC에서 바로 설치해요</span></p>
    </ProviderSetupCard>;
  }

  const checking = phase === "checking";
  return <ProviderSetupCard label={label} tone={error ? "error" : "install"} busy={checking}
    glyph={checking ? <LoaderCircle size={15} className="dc-setup-spin" />
      : error ? <CircleAlert size={15} strokeWidth={2.5} /> : <Download size={15} strokeWidth={2.5} />}
    actions={installable ? <>
      <button type="button" className="dc-setup-button" data-variant="ghost"
        onClick={() => void openHelp()}>직접 설치 방법</button>
      <button type="button" className="dc-setup-button" data-variant="primary"
        disabled={checking} onClick={() => void check()}>{checking ? "확인 중…" : "앱에서 설치하기"}</button>
    </> : <button type="button" className="dc-setup-button" data-variant="primary"
      onClick={() => void openHelp()}>설치 안내 열기<ExternalLink size={13} aria-hidden="true" /></button>}>
    <p className="dc-setup-text" role="status"><strong>{title} 없음</strong>
      <span className="dc-setup-muted">{installable ? "이 PC에서 찾지 못했어요"
        : "공식 안내대로 설치한 뒤 다시 확인해 주세요"}</span></p>
    {error && <p className="dc-setup-error" role="alert">{error}</p>}
  </ProviderSetupCard>;
}
