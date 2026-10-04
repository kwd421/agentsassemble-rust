import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { centralAccountEntryUrl } from "../../lib/centralIdentity";
import { CentralOwnerWorkspaceContext } from "../../lib/centralOwnerWorkspaceContext";
import { type CentralOwnerWorkspace } from "../../lib/centralOwnerWorkspace";
import type { CentralOwnerSessionStatus } from "../../types/generated/CentralOwnerSessionStatus";

export default function CentralOwnerWorkspaceBoundary({ session, children }: { session: CentralOwnerWorkspace; children: ReactNode }) {
  const [status, setStatus] = useState<CentralOwnerSessionStatus>({ state: "active" });
  const current = useRef(status);
  const onStatus = useCallback((next: CentralOwnerSessionStatus) => {
    if (current.current.state === "ended") return;
    current.current = next;
    setStatus(next);
  }, []);
  const context = useMemo(() => ({ session, onStatus }), [session, onStatus]);
  const ended = status?.state === "ended";
  return <CentralOwnerWorkspaceContext.Provider value={context}>
    <div inert={ended || undefined} aria-hidden={ended || undefined} className="contents">{children}</div>
    {ended && <EndedWorkspace reason={status.reason} />}
  </CentralOwnerWorkspaceContext.Provider>;
}

function EndedWorkspace({ reason }: { reason: "revoked" | "disconnected" | "unavailable" }) {
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
          ? "호스트에서 이 기기의 연결을 해제했어요."
          : reason === "disconnected" ? "호스트와의 연결이 끊겼어요."
          : "서버 접속 권한을 확인하지 못했어요."} 내 서버 목록에서 다시 연결해 주세요.</p>
        {centralAccountEntryUrl() && <a className="ops-button" href={centralAccountEntryUrl()}>내 서버 목록으로</a>}
      </main>
    </dialog>;
}
