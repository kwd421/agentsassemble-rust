import { Fragment, useEffect, useRef, useState } from "react";
import { ServerIcon } from "./CentralServerList";
import type { CentralDirectoryState } from "../../app/useCentralDirectory";
import type { CentralServerDisplay } from "../../lib/central/directoryCache";
import { CONNECTION_LABELS } from "../../lib/serverConnectionState";

export default function ServerRailEntries({ directory, localServerIds, connectingId, connectedServerId, onOpen, onHide }: {
  directory: CentralDirectoryState | null; localServerIds: string[]; connectingId: string;
  connectedServerId?: string;
  onOpen: (server: CentralServerDisplay) => Promise<void>;
  onHide?: (server: CentralServerDisplay) => Promise<void>;
}) {
  const [menu, setMenu] = useState<{server:CentralServerDisplay;x:number;y:number;trigger:HTMLElement} | null>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!menu) return;
    menuRef.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const close = () => { setMenu(null); menu.trigger.focus(); };
    const outside = (event: PointerEvent) => { if (!menuRef.current?.contains(event.target as Node)) close(); };
    const keyboard = (event: KeyboardEvent) => { if (event.key === "Escape" || event.key === "Tab") close(); };
    document.addEventListener("pointerdown",outside); document.addEventListener("keydown",keyboard);
    return () => { document.removeEventListener("pointerdown",outside);document.removeEventListener("keydown",keyboard); };
  },[menu]);
  if (!directory) return null;
  return <>{directory.servers.filter(server => !localServerIds.includes(server.server_id)).map(server => {
    const state = connectedServerId === server.server_id ? "connected" : connectingId === server.server_id ? "connecting" : directory.status === "central-unconfirmed" ? "central-unconfirmed" : "disconnected";
    const label = `${server.alias || "이름 없는 서버"} · ${CONNECTION_LABELS[state]}`;
    return <Fragment key={server.server_id}><button type="button" className="dc-server-btn" data-connection-state={state}
      onContextMenu={event => { if (server.relation !== "member" || !onHide) return; event.preventDefault(); setMenu({server,x:event.clientX,y:event.clientY,trigger:event.currentTarget}); }}
      aria-label={label} title={label} onClick={() => void onOpen(server)}>
      <span className="dc-server-initials" aria-hidden><ServerIcon reference={directory.status === "connected" ? server.icon : undefined} name={server.alias || "이름 없는 서버"} /></span>
      <span className="dc-server-connection-dot" aria-hidden />
    </button>{server.relation === "member" && onHide && <button className="dc-server-row-icon-button" aria-label={`${server.alias || "서버"} 메뉴`} onClick={event => {const rect=event.currentTarget.getBoundingClientRect();setMenu({server,x:rect.right,y:rect.top,trigger:event.currentTarget});}}>⋯</button>}</Fragment>;
  })}{menu && onHide && <div ref={menuRef} className="dc-context-menu" role="menu" aria-label="서버 메뉴" style={{left:Math.max(16,Math.min(menu.x,window.innerWidth-220)),top:Math.max(16,Math.min(menu.y,window.innerHeight-80))}}>
    <button role="menuitem" onClick={() => {setMenu(null); void onHide(menu.server);}}>목록에서 숨기기</button>
  </div>}</>;
}
