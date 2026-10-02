import type { HostDeviceInfo } from "../../types/generated/HostDeviceInfo";
import { useRef, useState } from "react";
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
  const [name, setName] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const operation = useRef(false);

  async function save() {
    const editing = servers.find((server) => server.server_id === editingId);
    if (!editing || busy || operation.current) return;
    operation.current = true; setSaving(true); setError("");
    try {
      await renameCentralServer(editing, name);
      setEditingId(null);
      await onRefresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "서버 이름을 저장하지 못했습니다.");
    } finally { operation.current = false; setSaving(false); }
  }

  return <div className="grid gap-3" aria-busy={saving}>
    {localHost && onOpenLocal && !servers.some((server) => server.server_id === localHost.server_id) &&
      <div className="grid gap-2 rounded-md bg-[#2b2d31] p-3">
        <strong className="break-all text-sm">{localHost.host_name} · 이 기기</strong>
        <span className="text-xs text-text-secondary" aria-label="호스트 운영체제">{OS_LABELS[localHost.host_os as keyof typeof OS_LABELS]}</span>
        <span className="text-xs text-text-muted">{localHost.server_id ? "이 계정에 등록되지 않은 로컬 서버" : "새 서버"}</span>
        <button type="button" className="ops-button w-fit" aria-label="이 기기 서버 열기" disabled={busy || saving} onClick={() => void onOpenLocal()}>서버 열기</button>
      </div>}
    {servers.map((server) => {
      const isLocal = Boolean(localHost?.server_id === server.server_id && onOpenLocal);
      const online = server.relation === "owner" && server.endpoint?.status === "likely_online" && server.endpoint.lease_expires_at > Date.now() / 1000;
      return <div key={server.server_id} className="grid gap-2 rounded-md bg-[#2b2d31] p-3">
        <strong className="break-all text-sm"><span>{server.alias || server.server_id}</span> · {server.server_id.slice(0, 8)}{isLocal && " · 이 기기"}</strong>
        <span className="w-fit rounded border border-white/15 bg-white/5 px-2 py-0.5 text-xs text-text-secondary" aria-label="호스트 운영체제">{server.host_os ? OS_LABELS[server.host_os] : "OS 미확인"}</span>
        <span className="text-xs text-text-muted">{server.relation !== "owner" ? "초대 링크로 접속해 주세요" : isLocal ? "이 기기에서 열 수 있습니다" : online ? "온라인" : "오프라인 · 호스트에서 서버를 열어 주세요"}</span>
        {editingId === server.server_id ? <form className="grid gap-2" onSubmit={(event) => { event.preventDefault(); void save(); }}>
          <label className="grid gap-1 text-sm">서버 이름
            <input autoFocus className="rounded bg-[#101114] p-2 text-text-primary" value={name} maxLength={80} disabled={busy || saving} onChange={(event) => setName(event.target.value)} />
          </label>
          <div className="flex gap-2">
            <button type="submit" className="ops-button" disabled={busy || saving || !name.trim()}>이름 저장</button>
            <button type="button" className="ops-button" disabled={saving} onClick={() => { setEditingId(null); setError(""); }}>취소</button>
          </div>
        </form> : <div className="flex gap-2">
          <button type="button" className="ops-button" aria-label={`${server.alias || server.server_id} 서버 열기`} disabled={busy || saving || (!isLocal && !online)} onClick={() => { if (isLocal && onOpenLocal) void onOpenLocal(); else void onOpen(server); }}>서버 열기</button>
          {server.relation === "owner" && <button type="button" className="ops-button" aria-label={`${server.alias || server.server_id} 이름 변경`} disabled={busy || saving} onClick={() => { setEditingId(server.server_id); setName(server.alias); setError(""); }}>이름 변경</button>}
        </div>}
      </div>;
    })}
    {error && <p role="alert" className="text-sm text-red-300">{error}</p>}
  </div>;
}
