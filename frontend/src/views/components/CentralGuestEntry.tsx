import { useState } from "react";
import { ArrowLeft, ArrowRight, Check, Copy, KeyRound, LoaderCircle, UserRound } from "../../ui/icons";
import { createCentralGuest, recoverCentralGuest, loadPendingCentralRecoveryCode, clearPendingCentralRecoveryCode } from "../../lib/central/identity";

export type CentralGuestScreen = "choice" | "guest" | "recover" | "recovery-code";

// Both trusted entry points use the same identity and recovery-code acknowledgement.
// Invitations remain with the caller; this component never accepts or persists them.
export default function CentralGuestEntry({ screen, setScreen, disabled = false, onComplete, onBusyChange }: {
  screen: CentralGuestScreen; setScreen: (screen: CentralGuestScreen) => void;
  disabled?: boolean; onComplete: () => Promise<void>; onBusyChange?: (busy: boolean) => void;
}) {
  const [displayName, setDisplayName] = useState("");
  const [recoveryInput, setRecoveryInput] = useState("");
  const [issuedRecoveryCode, setIssuedRecoveryCode] = useState(() => loadPendingCentralRecoveryCode() || "");
  const [savedRecoveryCode, setSavedRecoveryCode] = useState(false);
  const [copied, setCopied] = useState(false);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState("");
  const busy = disabled || working;
  function setBusy(value: boolean) { setWorking(value); onBusyChange?.(value); }
  function showCode(code: string) {
    setIssuedRecoveryCode(code); setSavedRecoveryCode(false); setCopied(false); setScreen("recovery-code");
  }
  async function submit(recover: boolean) {
    if (busy || !(recover ? recoveryInput : displayName).trim()) return;
    setBusy(true); setError("");
    try {
      const result = await (recover ? recoverCentralGuest(recoveryInput) : createCentralGuest(displayName.trim()));
      showCode(result.recovery_code);
    } catch {
      const pending = loadPendingCentralRecoveryCode();
      if (pending) showCode(pending);
      else setError(recover ? "게스트 신원을 복구하지 못했어요. 다시 시도해 주세요." : "게스트 신원을 만들지 못했어요. 다시 시도해 주세요.");
    } finally { setBusy(false); }
  }
  const createGuest = () => submit(false);
  const recoverGuest = () => submit(true);
  async function copyRecoveryCode() {
    try { await navigator.clipboard.writeText(issuedRecoveryCode); setCopied(true); }
    catch { setError("복사 권한이 거부됐어요. 코드를 직접 선택해 복사해 주세요."); }
  }
  async function continueAfterRecoveryCode() {
    if (!savedRecoveryCode || busy) return;
    setBusy(true); setError("");
    try { await onComplete(); clearPendingCentralRecoveryCode(); }
    catch { setError("다음 화면을 열지 못했어요. 다시 시도해 주세요."); }
    finally { setBusy(false); }
  }
  return <>
    {error && <p role="alert">{error}</p>}
    {/* Sits right under the Google button: the card's 20px gap minus 12px leaves
        the 8px Discord puts between sign-in buttons. */}
    {screen === "choice" && <div className="-mt-3 grid gap-3">
            {/* Google's own button keeps Google's branded style; ours is a Discord
                secondary button so the two read as different actions. */}
            <button
              type="button"
              className="dc-button dc-button--lg dc-button--secondary w-full"
              disabled={busy}
              onClick={() => {
                setError("");
                setScreen("guest");
              }}
            >
              <UserRound size={18} aria-hidden /> 게스트로 계속
            </button>
            <p className="dc-startup-recover-line">
              복구 코드가 있나요?{" "}
              <button
                type="button"
                className="dc-startup-link"
                disabled={busy}
                onClick={() => {
                  setError("");
                  setScreen("recover");
                }}
              >
                입력하기
              </button>
            </p>
    </div>}
        {screen === "guest" && (
          <section className="dc-startup-form">
            <button
              type="button"
              className="dc-startup-back"
              onClick={() => setScreen("choice")}
            >
              <ArrowLeft size={16} /> 뒤로
            </button>
            <label className="dc-startup-field">
              표시 이름
              <input
                autoFocus
                type="text"
                maxLength={80}
                value={displayName}
                placeholder="다른 참가자에게 보일 이름"
                className="dc-input"
                onChange={(event) => setDisplayName(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void createGuest();
                }}
              />
            </label>
            <button
              type="button"
              className="dc-button dc-button--lg dc-button--primary"
              disabled={!displayName.trim() || busy}
              onClick={() => void createGuest()}
            >
              {busy ? (
                <LoaderCircle size={16} className="animate-spin" />
              ) : (
                <ArrowRight size={16} />
              )}{" "}
              {busy ? "만드는 중…" : "게스트 만들기"}
            </button>
          </section>
        )}

        {screen === "recover" && (
          <section className="dc-startup-form">
            <button
              type="button"
              className="dc-startup-back"
              onClick={() => setScreen("choice")}
            >
              <ArrowLeft size={16} /> 뒤로
            </button>
            <label className="dc-startup-field">
              복구 코드
              <input
                autoFocus
                autoComplete="one-time-code"
                spellCheck={false}
                value={recoveryInput}
                placeholder="XXXX-XXXX-…"
                className="dc-input dc-input--code"
                onChange={(event) =>
                  setRecoveryInput(event.currentTarget.value.toUpperCase())
                }
              />
            </label>
            <button
              type="button"
              className="dc-button dc-button--lg dc-button--primary"
              disabled={!recoveryInput.trim() || busy}
              onClick={() => void recoverGuest()}
            >
              {busy ? (
                <LoaderCircle size={16} className="animate-spin" />
              ) : (
                <KeyRound size={16} />
              )}{" "}
              {busy ? "확인하는 중…" : "이 게스트로 계속"}
            </button>
          </section>
        )}

        {screen === "recovery-code" && (
          <section className="dc-startup-form">
            <label className="dc-startup-field">
              새 복구 코드
              <input
                readOnly
                value={issuedRecoveryCode}
                onFocus={(event) => event.currentTarget.select()}
                className="dc-input dc-input--code"
              />
            </label>
            <button
              type="button"
              className="dc-button dc-button--lg dc-button--secondary"
              onClick={() => void copyRecoveryCode()}
            >
              {copied ? <Check size={16} /> : <Copy size={16} />} {copied ? "복사됨" : "복구 코드 복사"}
            </button>
            <p className="dc-startup-warning">
              이 코드를 잃어버리면 다른 기기에서 이 게스트로 다시 들어올 수 없어요. 비밀번호 관리자처럼 안전한 곳에 보관해 주세요. 코드를 한 번 쓰면 새 코드로 바뀌어요.
            </p>
            <label className="flex items-start gap-2 text-[14px] leading-5 text-text-secondary">
              <input
                type="checkbox"
                className="dc-checkbox mt-1"
                checked={savedRecoveryCode}
                onChange={(event) => setSavedRecoveryCode(event.currentTarget.checked)}
              />
              복구 코드를 안전한 곳에 저장했어요.
            </label>
            <button
              type="button"
              className="dc-button dc-button--lg dc-button--primary"
              disabled={!savedRecoveryCode || busy}
              onClick={() => void continueAfterRecoveryCode()}
            >
              {busy ? <LoaderCircle size={16} className="animate-spin" /> : <ArrowRight size={16} />} 계속
            </button>
          </section>
        )}

  </>;
}
