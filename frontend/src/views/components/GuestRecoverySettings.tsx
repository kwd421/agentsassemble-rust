import { useLayoutEffect, useRef, useState } from "react";
import { Copy, KeyRound, Link2 } from "lucide-react";

import { issueGuestRecoveryCode, type UserProfileIdentity } from "../../api";

export default function GuestRecoverySettings({
  identity,
}: {
  identity: UserProfileIdentity;
}) {
  const [code, setCode] = useState("");
  const [recoveryUrl, setRecoveryUrl] = useState("");
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);

  const generation = useRef(0);
  const inFlight = useRef(false);
  useLayoutEffect(() => {
    generation.current += 1;
    inFlight.current = false;
    setCode("");
    setRecoveryUrl("");
    setStatus("");
    setBusy(false);
    return () => { generation.current += 1; };
  }, [identity.sessionToken, identity.deviceToken]);

  async function issue() {
    if (!identity.sessionToken || inFlight.current) return;
    const scope = generation.current;
    inFlight.current = true;
    setBusy(true);
    setCode("");
    setRecoveryUrl("");
    setStatus("복구 코드를 만드는 중...");
    try {
      const result = await issueGuestRecoveryCode({
        sessionToken: identity.sessionToken,
        deviceToken: identity.deviceToken,
      });
      if (scope !== generation.current) return;
      setCode(result.recovery_code);
      setRecoveryUrl(result.recovery_url);
      setStatus("새 코드를 만들었어요. 이전 코드는 더 이상 사용할 수 없어요.");
    } catch (error) {
      if (scope !== generation.current) return;
      setStatus(error instanceof Error ? error.message : "복구 코드를 만들지 못했습니다.");
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
      setStatus(`${label}를 복사했어요.`);
    } catch {
      if (scope !== generation.current) return;
      setStatus("값을 직접 선택해 복사해 주세요.");
    }
  }

  return (
    <div className="dc-guest-recovery-settings">
      <header>
        <KeyRound size={18} />
        <div>
          <h3>게스트 신원 복구</h3>
          <p>다른 기기에서도 지금 이름과 참여한 방을 이어갈 수 있어요.</p>
        </div>
      </header>
      <p className="dc-guest-recovery-warning">
        새 코드를 만들면 이전 코드는 더 이상 사용할 수 없어요.
      </p>
      <button style={{ minWidth: 44, minHeight: 44 }} type="button" className="dc-guest-recovery-issue" disabled={busy} onClick={() => void issue()}>
        <KeyRound size={16} />
        {busy ? "발급 중..." : code ? "새 코드로 교체" : "복구 코드 만들기"}
      </button>
      {code && (
        <div className="dc-guest-recovery-result">
          <label>
            일회용 복구 코드
            <span>
              <input style={{ minHeight: 44 }} value={code} readOnly spellCheck={false} />
              <button style={{ minWidth: 44, minHeight: 44 }} type="button" aria-label="복구 코드 복사" onClick={() => void copy(code, "복구 코드")}>
                <Copy size={16} />
              </button>
            </span>
          </label>
          <button style={{ minWidth: 44, minHeight: 44 }} type="button" onClick={() => void copy(recoveryUrl, "복구 링크")}>
            <Link2 size={16} />
            복구 링크 복사
          </button>
        </div>
      )}
      {status && <p className="dc-member-session-status preserve-words" role="status">{status}</p>}
    </div>
  );
}
