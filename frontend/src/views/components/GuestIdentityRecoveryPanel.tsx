import { useLayoutEffect, useRef, useState } from "react";
import { Check, Copy, KeyRound, LoaderCircle } from "lucide-react";

import {
  redeemGuestRecoveryCode,
  type GuestRecoveryRedeemResponse,
} from "../../api";
import type { GuestRecoveryRequest } from "../../lib/guestRecovery";
import { userFacingError } from "../../lib/apiErrors";

export default function GuestIdentityRecoveryPanel({
  deviceToken,
  clientId,
  request,
  onRecovered,
}: {
  deviceToken: string;
  clientId: string;
  request: GuestRecoveryRequest;
  onRecovered: (payload: GuestRecoveryRedeemResponse) => void;
}) {
  const [recoveryCode, setRecoveryCode] = useState(request.recoveryCode);
  const [recovered, setRecovered] = useState<GuestRecoveryRedeemResponse | null>(null);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);

  const generation = useRef(0);
  const inFlight = useRef(false);
  useLayoutEffect(() => {
    generation.current += 1;
    inFlight.current = false;
    setRecoveryCode(request.recoveryCode);
    setRecovered(null);
    setStatus("");
    setBusy(false);
    setCopied(false);
    return () => { generation.current += 1; };
  }, [deviceToken, clientId, request.roomId, request.recoveryCode]);

  async function recover() {
    if (inFlight.current) return;
    const scope = generation.current;
    inFlight.current = true;
    setBusy(true);
    setStatus("기존 신원과 방 멤버십을 확인하는 중...");
    try {
      const payload = await redeemGuestRecoveryCode({
        recoveryCode,
        roomId: request.roomId,
        deviceToken,
        clientId,
      });
      if (scope !== generation.current) return;
      setRecovered(payload);
      setRecoveryCode(payload.recovery_code);
      setStatus("");
    } catch (error) {
      if (scope !== generation.current) return;
      setStatus(userFacingError(error, "신원을 복구하지 못했어요."));
    } finally {
      if (scope === generation.current) {
        inFlight.current = false;
        setBusy(false);
      }
    }
  }

  async function copyReplacementCode() {
    if (!recovered) return;
    const scope = generation.current;
    try {
      await navigator.clipboard.writeText(recovered.recovery_code);
      if (scope !== generation.current) return;
      setCopied(true);
    } catch {
      if (scope !== generation.current) return;
      setStatus("복사하지 못했어요. 코드를 직접 선택해서 복사해 주세요.");
    }
  }

  return (
    <div className="dc-guest-join-panel">
      <section className="dc-guest-recovery-card" aria-label="복구 코드로 계속">
        <span className="dc-guest-recovery-icon" aria-hidden>
          {recovered ? <Check size={24} /> : <KeyRound size={24} />}
        </span>
        <div>
          <h1>{recovered ? "다시 이어서 쓸 수 있어요" : "복구 코드로 계속"}</h1>
          <p>
            {recovered
              ? `이 기기에서 ${recovered.display_name}(으)로 참여한 방을 이어서 써요.`
              : "복구 코드를 넣으면 전에 쓰던 게스트 이름과 참여한 방을 이 기기에서 이어서 써요."}
          </p>
        </div>

        <label className="dc-guest-recovery-code">
          {recovered ? "새 복구 코드" : "복구 코드"}
          <input
            style={{ minHeight: 44 }}
            value={recoveryCode}
            readOnly={busy || Boolean(recovered)}
            autoComplete="one-time-code"
            spellCheck={false}
            onChange={(event) => setRecoveryCode(event.currentTarget.value)}
          />
        </label>

        {recovered ? (
          <>
            <p className="dc-guest-recovery-warning">
              방금 쓴 코드는 이제 쓸 수 없어요. 다음에 쓸 새 코드를 안전한 곳에 보관해 주세요.
            </p>
            <div className="dc-guest-recovery-actions">
              <button style={{ minWidth: 44, minHeight: 44 }} type="button" onClick={() => void copyReplacementCode()}>
                <Copy size={16} />
                {copied ? "복사됨" : "새 코드 복사"}
              </button>
              <button style={{ minWidth: 44, minHeight: 44 }} type="button" onClick={() => onRecovered(recovered)}>
                방으로 계속
              </button>
            </div>
          </>
        ) : (
          <button
            type="button"
            className="dc-guest-recovery-submit"
            style={{ minWidth: 44, minHeight: 44 }}
            disabled={busy || !recoveryCode.trim()}
            onClick={() => void recover()}
          >
            {busy ? <LoaderCircle className="animate-spin" size={16} /> : <KeyRound size={16} />}
            계속
          </button>
        )}
        {status && <p className="dc-member-session-status preserve-words" role="status">{status}</p>}
      </section>
    </div>
  );
}
