import type { HostDeviceInfo } from "../../types/generated/HostDeviceInfo";
import { useRef, useState } from "react";
import { Pencil } from "lucide-react";
import { roomInitials } from "../../lib/roomAppearance";
import { renameCentralServer, type CentralServer } from "../../lib/centralIdentity";

const OS_LABELS = { macos: "macOS", windows: "Windows", linux: "Linux", other: "기타 OS" };

type Props = {
  servers: CentralServer[];
  busy: boolean;
  localHost?: HostDeviceInfo | null;
  onOpenLocal?: () => Promise<void>;
  onOpen: (server: CentralServer) => Promise<void>;
  onRefresh: () => Promise<void>;
};

export default function CentralServerList({ servers, busy, localHost, onOpenLocal, onOpen, onRefresh }: Props) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setName] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const operation = useRef(false);

  async function save() {
    const editing = servers.find((server) => server.server_id === editingId);
    if (!editing || busy || operation.current) return;
    operation.current = true; setSaving(true); setError("");
    try {
      await renameCentralServer(editing, editingName);
      setEditingId(null);
      await onRefresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "서버 이름을 저장하지 못했습니다.");
    } finally { operation.current = false; setSaving(false); }
  }

  return <div className="dc-server-list" aria-busy={saving}>
    {localHost && onOpenLocal && !servers.some((server) => server.server_id === localHost.server_id) &&
      <div className="dc-server-row" data-state="local">
        <span className="dc-server-row-icon" aria-hidden>{roomInitials(localHost.host_name)}</span>
        <div className="dc-server-row-copy">
          <strong className="break-all">{localHost.host_name} · 이 기기</strong>
          <span className="dc-server-row-meta">
            {localHost.server_id ? "이 계정에 아직 등록되지 않았어요" : "새 서버"} · <span aria-label="호스트 운영체제">{OS_LABELS[localHost.host_os as keyof typeof OS_LABELS]}</span>
          </span>
        </div>
        <button type="button" className="ops-cta dc-server-row-open" aria-label="이 기기 서버 열기" disabled={busy || saving} onClick={() => void onOpenLocal()}>열기</button>
      </div>}
    {servers.map((server) => {
      const isLocal = Boolean(localHost?.server_id === server.server_id && onOpenLocal);
      const online = server.relation === "owner" && server.endpoint?.status === "likely_online" && server.endpoint.lease_expires_at > Date.now() / 1000;
      const name = server.alias || server.server_id;
      const openable = isLocal || online;
      const state = server.relation !== "owner" ? "invited" : isLocal ? "local" : online ? "online" : "offline";
      return <div key={server.server_id} className="dc-server-row" data-state={state}>
        <span className="dc-server-row-icon" aria-hidden>{roomInitials(name)}</span>
        <div className="dc-server-row-copy">
          <strong className="break-all"><span>{name}</span>{isLocal && " · 이 기기"}</strong>
          <span className="dc-server-row-meta">
            <span className="dc-server-row-dot" aria-hidden />
            {state === "invited" ? "초대 링크로 접속해 주세요" : state === "local" ? "이 기기에서 열 수 있어요" : state === "online" ? "온라인" : "꺼져 있음"}
            {" · "}<span aria-label="호스트 운영체제">{server.host_os ? OS_LABELS[server.host_os] : "OS 미확인"}</span>
            {" · "}{server.server_id.slice(0, 8)}
          </span>
          {editingId === server.server_id && <form className="dc-server-row-edit" onSubmit={(event) => { event.preventDefault(); void save(); }}>
            <label className="grid gap-1 text-[13px] text-text-secondary">서버 이름
              <input autoFocus className="rounded-lg bg-[#1e1f22] px-3 py-2 text-[15px] text-text-primary" value={editingName} maxLength={80} disabled={busy || saving} onChange={(event) => setName(event.target.value)} />
            </label>
            <div className="flex gap-2">
              <button type="submit" className="ops-cta min-h-11 px-4" disabled={busy || saving || !editingName.trim()}>이름 저장</button>
              <button type="button" className="ops-button" disabled={saving} onClick={() => { setEditingId(null); setError(""); }}>취소</button>
            </div>
          </form>}
        </div>
        {editingId !== server.server_id && <div className="dc-server-row-actions">
          {server.relation === "owner" && <button type="button" className="dc-server-row-icon-button" aria-label={`${name} 이름 변경`} title="이름 변경" disabled={busy || saving} onClick={() => { setEditingId(server.server_id); setName(server.alias); setError(""); }}><Pencil size={16} /></button>}
          <button type="button" className={openable ? "ops-cta dc-server-row-open" : "ops-button dc-server-row-open"} aria-label={`${name} 서버 열기`} disabled={busy || saving || !openable} onClick={() => { if (isLocal && onOpenLocal) void onOpenLocal(); else void onOpen(server); }}>열기</button>
        </div>}
      </div>;
    })}
    {error && <p role="alert" className="text-sm text-red-300">{error}</p>}
  </div>;
}
