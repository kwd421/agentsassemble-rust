import { useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { Globe, Laptop, Monitor, RefreshCw, Smartphone } from "../../ui/icons";
import type { UserProfileIdentity } from "../../api";
import type { OwnerDeviceSession } from "../../types/generated/OwnerDeviceSession";
import type { RevokeOwnerDevices } from "../../types/generated/RevokeOwnerDevices";
import { listOwnerDevices, revokeOwnerDevices } from "../../api/ownerDevices";
import { CentralOwnerWorkspaceContext } from "../../lib/central/ownerWorkspaceContext";
import "./OwnerDevicesPanel.css";
import { userFacingError } from "../../lib/apiErrors";

type Selection = { request: RevokeOwnerDevices; name: string; current: boolean };

export default function OwnerDevicesPanel({ identity }: { identity: UserProfileIdentity }) {
  const credential = identity.centralSession;
  const deviceToken = identity.deviceToken;
  const authority = useMemo<UserProfileIdentity>(() => ({ centralSession: credential
    ? { sessionToken: credential.sessionToken, generation: credential.generation } : undefined, deviceToken }),
    [credential?.sessionToken, credential?.generation, deviceToken]);
  const workspace = useContext(CentralOwnerWorkspaceContext);
  const [sessions, setSessions] = useState<OwnerDeviceSession[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [selection, setSelection] = useState<Selection | null>(null);
  const loading = useRef<AbortController | null>(null);
  const refresh = useCallback(async () => {
    loading.current?.abort();
    const controller = new AbortController();
    loading.current = controller;
    setBusy(true); setError("");
    try {
      const result = await listOwnerDevices(authority, controller.signal);
      if (!controller.signal.aborted) setSessions(result.sessions);
    } catch (reason) {
      if (!controller.signal.aborted) setError(userFacingError(reason, "기기 목록을 불러오지 못했어요."));
    } finally {
      if (!controller.signal.aborted) setBusy(false);
    }
  }, [authority]);
  useEffect(() => { void refresh(); return () => loading.current?.abort(); }, [refresh]);
  const confirm = async () => {
    if (!selection || busy) return;
    setBusy(true); setError("");
    try {
      await revokeOwnerDevices(authority, selection.request);
      setSelection(null);
      if (selection.current && workspace) {
        workspace.onStatus({ state: "ended", reason: "revoked" });
        return;
      }
      await refresh();
    } catch (reason) {
      setError(userFacingError(reason, "기기 연결을 해제하지 못했어요."));
    } finally { setBusy(false); }
  };
  return <div className="owner-devices">
    <div className="owner-devices-heading">
      <p className="dc-user-settings-lead">이 서버에 연결된 기기예요. 쓰지 않는 기기는 연결을 해제할 수 있어요.</p>
      <button className="owner-devices-refresh" type="button" aria-label="기기 목록 새로고침" disabled={busy} onClick={() => void refresh()}><RefreshCw size={18} /></button>
    </div>
    {error && <p role="alert" className="owner-devices-error">{error}</p>}
    {!sessions && busy && <p role="status" className="owner-devices-caption">기기를 불러오는 중이에요.</p>}
    {sessions?.length === 0 && <p className="owner-devices-caption">연결된 기기가 없어요.</p>}
    <ul className="owner-devices-list">
      {sessions?.map(session => {
        const os = ({ macos: "macOS", windows: "Windows", linux: "Linux", ios: "iOS", android: "Android" } as Record<string, string>)[session.os.toLowerCase()] || session.os;
        const generatedName = `${session.browser} · ${session.os}`;
        const name = (session.device_name === generatedName ? session.browser : session.device_name) || session.browser || "이름 없는 기기";
        const Icon = session.kind === "host" ? Monitor : /Android|iOS/i.test(session.os) ? Smartphone : session.browser ? Globe : Laptop;
        return <li key={session.session_id}>
          <span className="owner-device-icon" aria-hidden><Icon size={26} /></span>
          <div className="owner-device-info">
            <div className="owner-device-name"><strong>{name}</strong>{session.current && <span className="owner-device-current">이 기기</span>}{session.kind === "host" && <span className="owner-device-tag">서버 컴퓨터</span>}</div>
            <p>{[os, session.kind === "pairing" ? "링크로 연결한 기기" : ""].filter(Boolean).join(" · ")}
              {session.last_connected_at !== null && <>{os || session.kind === "pairing" ? " · " : ""}마지막 접속 <time dateTime={new Date(session.last_connected_at * 1000).toISOString()} title={new Date(session.last_connected_at * 1000).toLocaleString("ko-KR", { dateStyle: "medium", timeStyle: "short" })}>{relativeTimeLabel(session.last_connected_at * 1000)}</time></>}</p>
            {session.connected === false && <p>지금은 접속해 있지 않아요.</p>}
          </div>
          {session.revocable && <button type="button" className="owner-device-revoke" disabled={busy} aria-label={`${name} 연결 해제`}
            onClick={() => setSelection({ request: { scope: "session", session_id: session.session_id }, name, current: session.current })}>연결 해제</button>}
        </li>;
      })}
    </ul>
    <button type="button" className="owner-devices-revoke-all" disabled={busy || !sessions?.some(row => row.revocable)}
      onClick={() => setSelection({ request: { scope: "all" }, name: "모든 기기", current: Boolean(identity.centralSession) })}>모든 기기 연결 해제</button>
    {selection && <RevokeConfirmation selection={selection} busy={busy} error={error} onCancel={() => setSelection(null)} onConfirm={() => void confirm()} />}
  </div>;
}

// Discord shows recency, not a timestamp: 방금 / N분 전 / N시간 전 / N일 전, then the date.
function relativeTimeLabel(ms: number, now = Date.now()): string {
  const minutes = Math.floor(Math.max(0, now - ms) / 60_000);
  if (minutes < 1) return "방금";
  if (minutes < 60) return `${minutes}분 전`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}시간 전`;
  const days = Math.floor(hours / 24);
  if (days < 30) return `${days}일 전`;
  return new Date(ms).toLocaleDateString("ko-KR", { dateStyle: "medium" });
}

function RevokeConfirmation({ selection, busy, error, onCancel, onConfirm }: {
  selection: Selection; busy: boolean; error: string; onCancel: () => void; onConfirm: () => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    const prior = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog.current?.showModal(); cancel.current?.focus();
    return () => { dialog.current?.close(); prior?.focus(); };
  }, []);
  return <dialog ref={dialog} className="owner-device-confirm" aria-label="기기 연결 해제 확인" onCancel={event => { event.preventDefault(); if (!busy) onCancel(); }}>
    <h3>{selection.name} 연결을 해제할까요?</h3>
    <p>{selection.request.scope === "all" ? "서버 컴퓨터를 뺀 모든 기기의 연결이" : "이 기기의 연결이"} 바로 끊겨요.{selection.current && " 지금 쓰는 이 기기도 끊겨요."} 다시 쓰려면 새로 연결해야 해요.</p>
    {error && <p role="alert" className="owner-devices-error">{error}</p>}
    <div><button ref={cancel} type="button" disabled={busy} onClick={onCancel}>취소</button><button type="button" disabled={busy} onClick={onConfirm}>{busy ? "해제하는 중…" : "연결 해제"}</button></div>
  </dialog>;
}
