import { useLayoutEffect, useRef, useState } from "react";
import { Copy, KeyRound, Link2 } from "../../ui/icons";

import { issueGuestRecoveryCode, type UserProfileIdentity } from "../../api";
import { userFacingError } from "../../lib/apiErrors";

export default function GuestRecoverySettings({
  identity,
}: {
  identity: UserProfileIdentity;
}) {
  const [code, setCode] = useState("");
  const [recoveryUrl, setRecoveryUrl] = useState("");
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [copiedLabel, setCopiedLabel] = useState("");
  const copiedTimer = useRef<number | undefined>(undefined);
  useLayoutEffect(() => () => {
    if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
  }, []);

  const generation = useRef(0);
  const inFlight = useRef(false);
  useLayoutEffect(() => {
    generation.current += 1;
    inFlight.current = false;
    setCode("");
    setRecoveryUrl("");
    setStatus("");
    setBusy(false);
    setCopiedLabel("");
    return () => { generation.current += 1; };
  }, [identity.sessionToken, identity.deviceToken]);

  async function issue() {
    if (!identity.sessionToken || inFlight.current) return;
    const scope = generation.current;
    inFlight.current = true;
    setBusy(true);
    setCode("");
    setRecoveryUrl("");
    setStatus("");
    try {
      const result = await issueGuestRecoveryCode({
        sessionToken: identity.sessionToken,
        deviceToken: identity.deviceToken,
      });
      if (scope !== generation.current) return;
      setCode(result.recovery_code);
      setRecoveryUrl(result.recovery_url);
      // The new code fields are the only feedback; the warning above already
      // says the old code stops working.
      setStatus("");
    } catch (error) {
      if (scope !== generation.current) return;
      setStatus(userFacingError(error, "복구 코드를 만들지 못했어요."));
    } finally {
      if (scope === generation.current) {
        inFlight.current = false;
        setBusy(false);
      }
    }
  }

  async function copy(value: string, label: string) {
    const scope = generation.current;
    try {
      await navigator.clipboard.writeText(value);
      if (scope !== generation.current) return;
      setCopiedLabel(label);
      if (copiedTimer.current !== undefined) window.clearTimeout(copiedTimer.current);
      copiedTimer.current = window.setTimeout(() => {
        if (scope === generation.current) setCopiedLabel("");
      }, 2000);
    } catch {
      if (scope !== generation.current) return;
      setStatus("복사하지 못했어요. 직접 선택해서 복사해 주세요.");
    }
  }

  return (
    <div className="dc-guest-recovery-settings">
      <header>
        <KeyRound size={18} />
        <div>
          <h3>복구 코드</h3>
          <p>복구 코드가 있으면 다른 기기에서도 지금 이름과 참여한 방을 이어서 쓸 수 있어요.</p>
        </div>
      </header>
      <p className="dc-guest-recovery-warning">
        새 코드를 만들면 이전 코드는 더 이상 사용할 수 없어요.
      </p>
      <button style={{ minWidth: 44, minHeight: 44 }} type="button" className="dc-guest-recovery-issue" disabled={busy} onClick={() => void issue()}>
        <KeyRound size={16} />
        {busy ? "만드는 중…" : code ? "새 코드 만들기" : "복구 코드 만들기"}
      </button>
      {code && (
        <div className="dc-guest-recovery-result">
          <label>
            일회용 복구 코드
            <span>
              <input className="dc-input font-mono" value={code} readOnly spellCheck={false} />
              <button style={{ minWidth: 44, minHeight: 44 }} type="button" aria-label={copiedLabel === "복구 코드" ? "복구 코드 복사됨" : "복구 코드 복사"} onClick={() => void copy(code, "복구 코드")}>
                <Copy size={16} />
                {copiedLabel === "복구 코드" ? "복사됨" : null}
              </button>
            </span>
          </label>
          <button style={{ minWidth: 44, minHeight: 44 }} type="button" onClick={() => void copy(recoveryUrl, "복구 링크")}>
            <Link2 size={16} />
            {copiedLabel === "복구 링크" ? "복사됨" : "복구 링크 복사"}
          </button>
        </div>
      )}
      {status && <p className="dc-member-session-status preserve-words" role="status">{status}</p>}
    </div>
  );
}
