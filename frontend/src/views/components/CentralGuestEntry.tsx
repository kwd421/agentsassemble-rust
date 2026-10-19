import { useState } from "react";
import { ArrowLeft, ArrowRight, Check, Copy, KeyRound, LoaderCircle, UserRound } from "lucide-react";
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
    {screen === "choice" && <div className="grid gap-3">
            <button
              type="button"
              className="dc-google-signin w-full"
              disabled={busy}
              onClick={() => {
                setError("");
                setScreen("guest");
              }}
            >
              <UserRound size={18} aria-hidden /> 새 게스트로 계속
            </button>
            <button
              type="button"
              className="min-h-10 text-[13px] font-medium text-[#aeb4ff] hover:underline"
              disabled={busy}
              onClick={() => {
                setError("");
                setScreen("recover");
              }}
            >
              이미 복구 코드가 있어요
            </button>
    </div>}
        {screen === "guest" && (
          <section className="grid gap-3 rounded-lg bg-[#24242a] p-4">
            <button
              type="button"
              className="flex w-fit items-center gap-1 text-[11px] font-black text-text-muted"
              onClick={() => setScreen("choice")}
            >
              <ArrowLeft size={14} /> 뒤로
            </button>
            <label className="grid gap-1.5 text-[11px] font-black text-text-secondary">
              표시 이름
              <input
                autoFocus
                type="text"
                maxLength={80}
                value={displayName}
                placeholder="다른 참가자에게 보일 이름"
                className="min-h-10 rounded-md border border-transparent bg-[#202024] px-3 text-[13px] font-semibold text-text-primary outline-none focus:border-[#5865f2]"
                onChange={(event) => setDisplayName(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") void createGuest();
                }}
              />
            </label>
            <button
              type="button"
              className="flex min-h-10 items-center justify-center gap-2 rounded-md bg-[#5865f2] px-4 text-[13px] font-black text-white disabled:opacity-50"
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
          <section className="grid gap-3 rounded-lg bg-[#24242a] p-4">
            <button
              type="button"
              className="flex w-fit items-center gap-1 text-[11px] font-black text-text-muted"
              onClick={() => setScreen("choice")}
            >
              <ArrowLeft size={14} /> 뒤로
            </button>
            <label className="grid gap-1.5 text-[11px] font-black text-text-secondary">
              게스트 복구 코드
              <input
                autoFocus
                autoComplete="one-time-code"
                spellCheck={false}
                value={recoveryInput}
                placeholder="XXXX-XXXX-…"
                className="min-h-10 rounded-md bg-[#202024] px-3 font-mono text-[13px] text-text-primary outline-none"
                onChange={(event) =>
                  setRecoveryInput(event.currentTarget.value.toUpperCase())
                }
              />
            </label>
            <button
              type="button"
              className="flex min-h-10 items-center justify-center gap-2 rounded-md bg-[#5865f2] px-4 text-[13px] font-black text-white disabled:opacity-50"
              disabled={!recoveryInput.trim() || busy}
              onClick={() => void recoverGuest()}
            >
              {busy ? (
                <LoaderCircle size={16} className="animate-spin" />
              ) : (
                <KeyRound size={16} />
              )}{" "}
              {busy ? "복구 중…" : "같은 게스트로 로그인"}
            </button>
          </section>
        )}

        {screen === "recovery-code" && (
          <section className="grid gap-4 rounded-lg bg-[#24242a] p-4">
            <label className="grid gap-2 text-[11px] font-black text-text-secondary">
              새 복구 코드
              <input
                readOnly
                value={issuedRecoveryCode}
                onFocus={(event) => event.currentTarget.select()}
                className="min-h-12 rounded-md bg-[#202024] px-3 font-mono text-[13px] font-black tracking-wide text-text-primary outline-none"
              />
            </label>
            <button
              type="button"
              className="flex min-h-10 items-center justify-center gap-2 rounded-md bg-[#323237] px-4 text-[12px] font-black text-text-primary"
              onClick={() => void copyRecoveryCode()}
            >
              {copied ? <Check size={16} /> : <Copy size={16} />} {copied ? "복사됨" : "복구 코드 복사"}
            </button>
            <p className="rounded-md bg-[#3a2526] p-3 text-[11px] font-bold leading-5 text-[#ffb4b5]">
              이 코드를 잃으면 다른 기기에서 이 게스트 신원을 복구할 수 없어요. 비밀번호 관리자나 안전한 오프라인 장소에 보관하세요. 복구 직후에는 이전 코드가 폐기돼요.
            </p>
            <label className="flex items-start gap-2 text-[12px] font-bold leading-5 text-text-secondary">
              <input
                type="checkbox"
                className="mt-1"
                checked={savedRecoveryCode}
                onChange={(event) => setSavedRecoveryCode(event.currentTarget.checked)}
              />
              복구 코드를 안전한 곳에 저장했어요.
            </label>
            <button
              type="button"
              className="flex min-h-10 items-center justify-center gap-2 rounded-md bg-[#5865f2] px-4 text-[13px] font-black text-white disabled:opacity-50"
              disabled={!savedRecoveryCode || busy}
              onClick={() => void continueAfterRecoveryCode()}
            >
              {busy ? <LoaderCircle size={16} className="animate-spin" /> : <ArrowRight size={16} />} 계속
            </button>
          </section>
        )}

  </>;
}
