import { ServerIcon } from "./CentralServerList";
import type { CentralDirectoryState } from "../../app/useCentralDirectory";
import type { CentralServerDisplay } from "../../lib/centralDirectoryCache";
import { CONNECTION_LABELS } from "../../lib/serverConnectionState";

export default function ServerRailEntries({ directory, localServerIds, connectingId, onOpen }: {
  directory: CentralDirectoryState | null; localServerIds: string[]; connectingId: string;
  onOpen: (server: CentralServerDisplay) => Promise<void>;
}) {
  if (!directory) return null;
  return <>{directory.servers.filter(server => !localServerIds.includes(server.server_id)).map(server => {
    const state = connectingId === server.server_id ? "connecting" : directory.status === "central-unconfirmed" ? "central-unconfirmed" : "disconnected";
    const label = `${server.alias || server.server_id} · ${CONNECTION_LABELS[state]}`;
    return <button key={server.server_id} type="button" className="dc-server-btn" data-connection-state={state}
      aria-label={label} title={label} onClick={() => void onOpen(server)}>
      <span className="dc-server-initials" aria-hidden><ServerIcon reference={directory.status === "connected" ? server.icon : undefined} name={server.alias || server.server_id} /></span>
      <span className="dc-server-connection-dot" aria-hidden />
    </button>;
  })}</>;
}
