import type { CentralServerDisplay } from "../../lib/central/directoryCache";
import type { HostDeviceInfo } from "../../types/generated/HostDeviceInfo";
import { useEffect, useRef, useState } from "react";
import { Camera, Pencil } from "lucide-react";
import { roomInitials } from "../../lib/roomAppearance";
import {
  fetchCentralServerIcon,
  renameCentralServer,
  setCentralServerIcon,
  type CentralServer,
} from "../../lib/central/identity";
import { loadRoomDockItems } from "../../lib/roomDockPersistence";
import ImageCropDialog from "./ImageCropDialog";

const OS_LABELS = { macos: "macOS", windows: "Windows", linux: "Linux", other: "기타 OS" };

type Props = {
  servers: CentralServerDisplay[];
  liveServers: CentralServer[];
  centralUnavailable?: boolean;
  connectingServerId?: string;
  busy: boolean;
  localHost?: HostDeviceInfo | null;
  profileName?: string;
  onOpenLocal?: (name?: string) => Promise<void>;
  onOpen: (server: CentralServerDisplay) => Promise<void>;
  onRefresh: () => Promise<void>;
};

// Icons are signed central resources: fetch once per reference and show a local URL.
// A failed fetch falls back to the initials and says so on hover.
export function ServerIcon({ reference, name }: { reference?: string; name: string }) {
  const [url, setUrl] = useState("");
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    setUrl("");
    setFailed(false);
    if (!reference) return;
    let objectUrl = "";
    let current = true;
    fetchCentralServerIcon(reference)
      .then((blob) => {
        if (!current) return;
        objectUrl = URL.createObjectURL(blob);
        setUrl(objectUrl);
      })
      .catch(() => { if (current) setFailed(true); });
    return () => {
      current = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [reference]);
  return url
    ? <img className="dc-server-row-icon-image" src={url} alt="" />
    : <span title={failed ? "아이콘을 불러오지 못했어요" : undefined}>{roomInitials(name)}</span>;
}

export default function CentralServerList({ servers, busy, localHost, profileName, onOpenLocal, onOpen, onRefresh, liveServers, centralUnavailable = false, connectingServerId }: Props) {
  const [localName, setLocalName] = useState<string | undefined>();
  const localProfileName = localHost?.profile_name || profileName;
  const localDefaultName = localHost && localProfileName ? `${localProfileName}의 ${localHost.device_kind}` : "";
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setName] = useState("");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const operation = useRef(false);
  const iconInputRef = useRef<HTMLInputElement>(null);
  const [iconTarget, setIconTarget] = useState<CentralServerDisplay | null>(null);
  const [iconFile, setIconFile] = useState<File | null>(null);
  const [iconStatus, setIconStatus] = useState("");

  function pickIcon(server: CentralServerDisplay) {
    setIconTarget(server);
    setError("");
    iconInputRef.current?.click();
  }

  async function applyIcon(server: CentralServerDisplay, icon: File | null) {
    if (busy || operation.current) return;
    operation.current = true; setSaving(true); setIconStatus(icon ? "아이콘 저장 중..." : ""); setError("");
    try {
      await setCentralServerIcon(server, icon);
      setIconFile(null); setIconTarget(null); setIconStatus("");
      await onRefresh();
    } catch (reason) {
      const message = reason instanceof Error ? reason.message : "서버 아이콘을 저장하지 못했어요.";
      if (icon) setIconStatus(message); else setError(message);
    } finally { operation.current = false; setSaving(false); }
  }

  async function save(reset = false) {
    if (busy || operation.current) return;
    if (editingId === "local") {
      setLocalName(reset ? undefined : editingName.trim());
      setEditingId(null); setError("");
      return;
    }
    const editing = servers.find((server) => server.server_id === editingId);
    if (!editing) return;
    operation.current = true; setSaving(true); setError("");
    try {
      await renameCentralServer(editing, reset ? null : editingName);
      setEditingId(null);
      await onRefresh();
    } catch (reason) {
      setError(reason instanceof Error ? reason.message : "서버 이름을 저장하지 못했어요.");
    } finally { operation.current = false; setSaving(false); }
  }

  function namePreview(name: string, serverId?: string | null) {
    const roomName = loadRoomDockItems().find(room => serverId && room.serverId === serverId)?.label || "새 회의실";
    return <p className="dc-server-name-preview text-xs text-text-muted preserve-words">
      초대받은 사람에게는 이렇게 보여요<br />‘{roomName}’ · {name}에서 열린 방
    </p>;
  }

  function nameEditor(defaultName?: string, server?: CentralServerDisplay) {
    return <form className="dc-server-row-edit" onSubmit={(event) => { event.preventDefault(); void save(); }}>
      <label className="grid gap-1 text-[13px] text-text-secondary">서버 이름
        <input autoFocus className="rounded-lg bg-[#1e1f22] px-3 py-2 text-[15px] text-text-primary" value={editingName} maxLength={80} disabled={busy || saving} onChange={(event) => setName(event.target.value)} />
      </label>
      {namePreview(editingName.trim(), server?.server_id || localHost?.server_id)}
      <div className="flex flex-wrap items-center gap-2">
        <div className="flex items-center gap-2">
          <button type="submit" className="ops-cta min-h-11 px-4" disabled={busy || saving || !editingName.trim() || editingName.trim().length > 80}>이름 저장</button>
          <button type="button" className="ops-button" disabled={saving} onClick={() => { setEditingId(null); setError(""); }}>취소</button>
        </div>
        {defaultName && (editingName.trim() !== defaultName || (server ? server.name_is_default === false : Boolean(localName))) && <button type="button" className="dc-server-row-text-button" disabled={busy || saving}
          onClick={() => void save(true)}>기본 이름으로 되돌리기</button>}
        {server?.icon && <button type="button" className="dc-server-row-text-button" disabled={busy || saving}
          onClick={() => void applyIcon(server, null)}>아이콘 제거</button>}
      </div>
    </form>;
  }

  return <div className="dc-server-list" aria-busy={saving}>
    {localHost && onOpenLocal && !servers.some((server) => server.server_id === localHost.server_id) &&
      <div className="dc-server-row" data-state="local">
        <span className="dc-server-row-icon" aria-hidden>{roomInitials(localName || localDefaultName)}</span>
        <div className="dc-server-row-copy">
          <strong className="break-all">{localName || localDefaultName || "프로필 이름 확인 중"}</strong>
          <span className="dc-server-row-meta">
            <span className="dc-server-row-dot" aria-hidden />이 기기 · <span aria-label="호스트 운영체제">{OS_LABELS[localHost.host_os as keyof typeof OS_LABELS]}</span>
          </span>
          {editingId === "local" && nameEditor(localDefaultName)}
        </div>
        {editingId !== "local" && <div className="dc-server-row-actions">
          <button type="button" className="dc-server-row-icon-button" aria-label={`${localName || localDefaultName} 이름 변경`} title="이름 변경" disabled={busy || saving || !localDefaultName}
            onClick={() => { setEditingId("local"); setName(localName || localDefaultName); setError(""); }}><Pencil size={16} /></button>
          <button type="button" className="ops-cta dc-server-row-open" aria-label="이 기기 서버 열기" disabled={busy || saving || !localDefaultName} onClick={() => void onOpenLocal(localName)}>열기</button>
        </div>}
      </div>}
    {servers.map((server) => {
      const isLocal = Boolean(localHost?.server_id === server.server_id && onOpenLocal);
      const live = liveServers.find(item => item.server_id === server.server_id);
      const online = !centralUnavailable && server.relation === "owner" && live?.endpoint?.status === "likely_online" && live.endpoint.lease_expires_at > Date.now() / 1000;
      const name = server.alias || "이름 없는 서버";
      const openable = isLocal || online;
      const state = connectingServerId === server.server_id ? "connecting" : isLocal ? "local" : centralUnavailable ? "central-unconfirmed" : server.relation !== "owner" ? "invited" : online ? "online" : "offline";
      return <div key={server.server_id} className="dc-server-row" data-state={state}>
        {server.relation === "owner"
          ? <button type="button" className="dc-server-row-icon dc-server-row-icon-edit" aria-label={`${name} 아이콘 변경`} title="아이콘 변경"
              disabled={busy || saving || centralUnavailable} onClick={() => pickIcon(server)}>
              <ServerIcon reference={centralUnavailable ? undefined : server.icon} name={name} />
              <span className="dc-server-row-icon-overlay" aria-hidden><Camera size={16} /></span>
            </button>
          : <span className="dc-server-row-icon" aria-hidden><ServerIcon reference={centralUnavailable ? undefined : server.icon} name={name} /></span>}
        <div className="dc-server-row-copy">
          <strong className="break-all">{name}</strong>
          <span className="dc-server-row-meta">
            <span className="dc-server-row-dot" aria-hidden />
            {state === "connecting" ? "연결 중" : state === "central-unconfirmed" ? "연결 끊김 · 로그인 서버 확인 불가" : state === "invited" ? "초대 링크로 접속해 주세요" : state === "local" ? "이 기기" : state === "online" ? "연결 가능" : "연결 끊김"}
            {" · "}<span aria-label="호스트 운영체제">{server.host_os ? OS_LABELS[server.host_os] : "OS 미확인"}</span>
          </span>
          {editingId === server.server_id && nameEditor(server.default_name, server)}
        </div>
        {editingId !== server.server_id && <div className="dc-server-row-actions">
          {server.relation === "owner" && <button type="button" className="dc-server-row-icon-button" aria-label={`${name} 이름 변경`} title="이름 변경" disabled={busy || saving || centralUnavailable} onClick={() => { setEditingId(server.server_id); setName(server.alias); setError(""); }}><Pencil size={16} /></button>}
          <button type="button" className={openable ? "ops-cta dc-server-row-open" : "ops-button dc-server-row-open"} aria-label={`${name} 서버 열기`} disabled={busy || saving || !openable} onClick={() => { if (isLocal && onOpenLocal) void onOpenLocal(); else void onOpen(server); }}>열기</button>
        </div>}
      </div>;
    })}
    {error && <p role="alert" className="text-sm text-red-300">{error}</p>}
    <input ref={iconInputRef} type="file" accept="image/*" hidden aria-label="서버 아이콘 이미지 선택"
      onChange={(event) => {
        const file = event.currentTarget.files?.[0] || null;
        event.currentTarget.value = "";
        if (file) { setIconStatus(""); setIconFile(file); }
      }} />
    {iconTarget && iconFile && <ImageCropDialog title="서버 아이콘 편집" file={iconFile} shape="square" busy={saving} status={iconStatus}
      onCancel={() => { setIconFile(null); setIconTarget(null); setIconStatus(""); }}
      onApply={(file) => void applyIcon(iconTarget, file)} />}
  </div>;
}
