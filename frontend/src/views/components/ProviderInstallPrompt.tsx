import { useEffect, useRef, useState, type ReactNode } from "react";
import { CircleCheck, LoaderCircle, PackagePlus } from "lucide-react";
import { ApiError } from "../../lib/apiErrors";
import { providerInstallOperation } from "../../api/providerOperations";
import { openProviderSetupHelp } from "../../lib/desktopBridge";
import type { ProviderInstall } from "../../types/generated/ProviderInstall";

// Owner responses that prove no installer is still running for this request.
const TERMINAL_INSTALL_FAILURES = [
  "provider_install_unsupported", "provider_install_already_installed", "provider_install_npm_missing",
  "provider_install_offer_changed", "provider_install_unavailable", "provider_install_invalid",
  "provider_install_unconfirmed", "provider_install_outside_path", "provider_install_catalog_unavailable",
];

/**
 * Offers to install a missing provider CLI. Nothing runs until the user reads the exact
 * command in a confirmation and accepts it; the runtime then installs only that version.
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
  const [phase, setPhase] = useState<"idle" | "checking" | "confirming" | "installing" | "done">("idle");
  const [error, setError] = useState("");
  const inflight = useRef(false);
  const busy = phase === "checking" || phase === "installing";
  const heading = `provider-install-${providerId}`;

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

  async function openHelp() {
    try {
      await openProviderSetupHelp(providerId);
    } catch (failure) {
      setError(failure instanceof Error ? failure.message : "공식 안내를 열지 못했어요.");
    }
  }

  function dismissOffer() {
    setOffer(null);
    setPhase("idle");
  }

  return <>
    <section className="dc-invite-hosting" aria-labelledby={heading}
      data-state={busy ? "busy" : phase === "done" ? "public" : "local"}>
      <span className="dc-invite-hosting-icon" aria-hidden="true">
        {busy ? <LoaderCircle className="dc-invite-hosting-spinner" size={22} />
          : phase === "done" ? <CircleCheck size={22} /> : <PackagePlus size={22} />}
      </span>
      <div className="dc-invite-hosting-copy">
        <div className="dc-invite-hosting-title-row">
          <h3 id={heading}>{displayName} CLI</h3>
          <span className="dc-invite-hosting-state">
            {phase === "installing" ? "설치 중" : phase === "checking" ? "확인 중"
              : phase === "done" ? "설치 완료" : "설치 필요"}
          </span>
        </div>
        <p style={{ whiteSpace: "normal", overflow: "visible", overflowWrap: "anywhere" }} role="status">
          {phase === "installing" ? "설치하고 있어요. 몇 분 걸릴 수 있어요."
            : phase === "done" ? `${displayName} CLI ${offer?.version}을 설치했어요.`
              : installable
                ? "이 PC에서 찾지 못했어요. 앱에서 설치하거나 공식 안내를 따라 설치할 수 있어요."
                : "이 PC에서 찾지 못했어요. 공식 안내를 따라 설치한 뒤 상태를 다시 확인해 주세요."}
        </p>
        {error && <span className="mt-1 text-[12px] font-bold text-offline preserve-words" role="alert">{error}</span>}
      </div>
      {phase !== "done" && <div className="dc-invite-hosting-actions">
        <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }}
          disabled={busy || !installable} onClick={() => void check()} hidden={!installable}>
          {phase === "checking" ? "확인 중…" : "앱에서 설치하기"}
        </button>
        {!installable && <button type="button" className="dc-invite-copy-button" style={{ minHeight: 44 }}
          onClick={() => void openHelp()}>설치 안내 열기</button>}
      </div>}
    </section>
    {installable && phase !== "done" && <button type="button" className="dc-agent-create-secondary preserve-words"
      style={{ minHeight: 40, marginTop: 8 }} onClick={() => void openHelp()}>
      직접 설치하는 방법 보기
    </button>}
    {phase === "confirming" && offer && <InstallConfirmation heading={`${heading}-confirm`} onCancel={dismissOffer}>
      <h3 id={`${heading}-confirm`}>{displayName} CLI를 설치할까요?</h3>
      <p>버전 {offer.version} · {offer.package}</p>
      <label className="dc-invite-command-label" htmlFor={`${heading}-command`}>이 PC에서 실행할 명령</label>
      <input id={`${heading}-command`} className="dc-invite-link-input" style={{ width: "100%" }}
        readOnly value={offer.command.join(" ")} onFocus={(event) => event.currentTarget.select()} />
      <div className="dc-invite-confirm-actions">
        <button type="button" className="dc-agent-create-secondary" style={{ minWidth: 44, minHeight: 44 }}
          autoFocus onClick={dismissOffer}>취소</button>
        <button type="button" className="dc-invite-confirm-primary" style={{ minWidth: 44, minHeight: 44 }}
          onClick={() => void install()}>설치</button>
      </div>
    </InstallConfirmation>}
  </>;
}

function InstallConfirmation({ heading, onCancel, children }: {
  heading: string;
  onCancel: () => void;
  children: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const [opener] = useState(() => document.activeElement);
  useEffect(() => {
    const dialog = ref.current;
    dialog?.showModal();
    return () => { dialog?.close(); if (opener instanceof HTMLElement && opener.isConnected) opener.focus(); };
  }, [opener]);
  return <dialog ref={ref} className="dc-invite-confirm" role="alertdialog" aria-labelledby={heading}
    onCancel={(event) => { event.preventDefault(); onCancel(); }}>
    {children}
  </dialog>;
}
