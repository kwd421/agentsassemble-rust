import { useRef, useState } from "react";
import { GoogleRegistrationRequired } from "../../lib/central/googleRegistration";
import GoogleGLogo from "./GoogleGLogo";
import { userFacingError } from "../../lib/apiErrors";

export default function GoogleRegistrationChoice({ request, onComplete, onCancel }: { request: GoogleRegistrationRequired; onComplete: () => Promise<void>; onCancel: () => void }) {
  const [busy, setBusy] = useState(false), [error, setError] = useState("");
  const operation = useRef(false);
  async function register() {
    if (operation.current) return;
    operation.current = true; setBusy(true); setError("");
    try { await request.register(); await onComplete(); }
    catch (reason) { setError(userFacingError(reason, "새 계정을 만들지 못했어요.")); }
    finally { operation.current = false; setBusy(false); }
  }
  return <section aria-label="Google 새 계정 만들기" aria-busy={busy} className="grid gap-5">
    <h1 className="text-2xl font-black text-text-primary">{request.identityStatus === "deleted" ? "탈퇴한 계정이에요" : "새 계정을 만들어 주세요"}</h1>
    <p className="text-[13px] font-semibold leading-5 text-text-muted">{request.message}</p>
    <button type="button" className="dc-button dc-button--primary dc-button--lg min-h-12 px-4" disabled={busy} onClick={() => void register()}>새 계정 만들기</button>
    <button type="button" className="dc-google-signin min-h-12 px-4" disabled={busy} onClick={onCancel}><GoogleGLogo />다른 계정으로 로그인</button>
    {error && <p role="alert">{error}</p>}
  </section>;
}
