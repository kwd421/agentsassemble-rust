import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { centralAccountEntryUrl } from "../../lib/centralIdentity";
import { CentralOwnerWorkspaceContext } from "../../lib/centralOwnerWorkspaceContext";
import { persistCentralOwnerWorkspace, type CentralOwnerWorkspace } from "../../lib/centralOwnerWorkspace";
import type { CentralOwnerSessionStatus } from "../../types/generated/CentralOwnerSessionStatus";

export default function CentralOwnerWorkspaceBoundary({ session, children }: { session: CentralOwnerWorkspace; children: ReactNode }) {
  const [status, setStatus] = useState<CentralOwnerSessionStatus>({ state: "active", expires_at: session.leaseExpiresAt });
  const current = useRef(status);
  const onStatus = useCallback((next: CentralOwnerSessionStatus) => {
    if (current.current.state === "ended") return;
    persistCentralOwnerWorkspace(next.state === "ended" ? null : { ...session, leaseExpiresAt: next.expires_at });
    current.current = next;
    setStatus(next);
  }, [session]);
  const context = useMemo(() => ({ session, onStatus }), [session, onStatus]);
  useEffect(() => {
    if (!status || status.state === "ended") return;
    const expire = () => {
      if (Date.now() >= status.expires_at * 1000) onStatus({ state: "ended", reason: "unavailable" });
    };
    const timeout = window.setTimeout(expire, Math.max(0, status.expires_at * 1000 - Date.now()));
    window.addEventListener("focus", expire);
    return () => { window.clearTimeout(timeout); window.removeEventListener("focus", expire); };
  }, [onStatus, status]);
  const ended = status?.state === "ended";
  return <CentralOwnerWorkspaceContext.Provider value={context}>
    <div inert={ended || undefined} aria-hidden={ended || undefined} className="contents">{children}</div>
    {ended && <EndedWorkspace reason={status.reason} />}
  </CentralOwnerWorkspaceContext.Provider>;
}

function EndedWorkspace({ reason }: { reason: "revoked" | "expired" | "unavailable" }) {
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    const element = dialog.current;
    element?.showModal();
    return () => element?.close();
  }, []);
  return <dialog ref={dialog} aria-modal="true" aria-label="서버 연결 종료"
    className="fixed inset-0 grid place-items-center bg-[#101114]/95 p-5"
    style={{ margin: 0, width: "100vw", height: "100dvh", maxWidth: "none", maxHeight: "none" }}
    onCancel={event => event.preventDefault()}>
      <main className="grid w-full max-w-[520px] gap-5 rounded-xl border border-white/10 bg-[#202126] p-6">
        <h1 className="text-xl font-bold text-text-primary">서버 연결이 종료됐어요</h1>
        <p role="alert" className="text-sm text-text-muted">{reason === "revoked"
          ? "로그아웃했거나 서버 접속 권한이 변경됐어요."
          : reason === "expired" ? "계정 세션이 만료됐어요."
          : "서버 접속 권한을 확인하지 못했어요."} 내 서버 목록에서 다시 연결해 주세요.</p>
        {centralAccountEntryUrl() && <a className="ops-button" href={centralAccountEntryUrl()}>내 서버 목록으로</a>}
      </main>
    </dialog>;
}
