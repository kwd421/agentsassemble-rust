import { useRef, useState } from "react";
import { registerFreshLocalHost } from "../../lib/central/freshHostRegistration";

export default function FreshHostRegistrationButton({ serverId, deviceToken, disabled, onRegistered }: { serverId: string; deviceToken: string; disabled: boolean; onRegistered: () => Promise<void> }) {
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const operation = useRef(false);
  async function register() {
    if (disabled || operation.current) return;
    operation.current = true; setBusy(true); setError("");
    try { await registerFreshLocalHost(serverId, deviceToken); await onRegistered(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "새 서버를 등록하지 못했어요."); }
    finally { operation.current = false; setBusy(false); }
  }
  return <section aria-label="새 계정으로 서버 등록" aria-busy={busy}>
    <p>이 컴퓨터는 이전 계정의 탈퇴로 외부 연결이 중지됐어요. 이전 계정의 정리가 끝나면 새 계정으로 서버를 다시 등록할 수 있어요. 이전 계정의 연결 관계는 이어받지 않아요.</p>
    <button type="button" className="ops-button" disabled={disabled || busy} onClick={() => void register()}>{busy ? "등록 중…" : "새 계정으로 이 컴퓨터의 서버 등록"}</button>
    {error && <p role="alert">{error}</p>}
  </section>;
}
