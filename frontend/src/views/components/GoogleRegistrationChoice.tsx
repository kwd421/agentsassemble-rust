import { useRef, useState } from "react";
import { GoogleRegistrationRequired } from "../../lib/central/googleRegistration";

export default function GoogleRegistrationChoice({ request, onComplete, onCancel }: { request: GoogleRegistrationRequired; onComplete: () => Promise<void>; onCancel: () => void }) {
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const operation = useRef(false);
  async function register() {
    if (operation.current) return;
    operation.current = true; setBusy(true); setError("");
    try { await request.register(); await onComplete(); }
    catch (reason) { setError(reason instanceof Error ? reason.message : "새 계정을 만들지 못했어요."); }
    finally { operation.current = false; setBusy(false); }
  }
  return <section aria-label="Google 새 계정 만들기" aria-busy={busy}>
    <p>{request.message}</p>
    <button type="button" className="ops-button" disabled={busy} onClick={() => void register()}>새 계정 만들기</button>
    <button type="button" className="ops-button" disabled={busy} onClick={onCancel}>취소</button>
    {error && <p role="alert">{error}</p>}
  </section>;
}
