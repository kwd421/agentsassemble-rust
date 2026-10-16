import { useRef, useState } from "react";
import { openCentralOwnedServer, openCentralMemberServer } from "../lib/central/identity";
import type { CentralServerDisplay } from "../lib/central/directoryCache";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { CentralDirectoryState } from "./useCentralDirectory";
import type { RoomDirectoryContinuity } from "./useRoomDirectory";

/**
 * Opening servers from the rail: owned servers through the consent gate,
 * member servers through explicit member consent. Anchored member hosts
 * instead re-enter silently with a fresh grant; only never-joined servers
 * go through explicit consent again.
 */
export function useServerOpening({
  refreshCentralDirectory,
  refreshRoomDirectory,
  captureRoomDirectoryContinuity,
  hasMemberAnchor,
  retryMemberHost,
}: {
  refreshCentralDirectory: () => Promise<CentralDirectoryState>;
  refreshRoomDirectory: (predecessor: RoomDirectoryContinuity) => Promise<{ ok: boolean }>;
  captureRoomDirectoryContinuity: () => RoomDirectoryContinuity;
  hasMemberAnchor: (serverId: string) => boolean;
  retryMemberHost: (serverId: string) => void;
}) {
  const [connectingServerId, setConnectingServerId] = useState("");
  const [serverConnectionError, setServerConnectionError] = useState("");
  const serverOpening = useRef(false);
  async function openRailServer(server: CentralServerDisplay) {
    if (serverOpening.current) return;
    serverOpening.current = true; setConnectingServerId(server.server_id); setServerConnectionError("");
    try {
      const current = await refreshCentralDirectory();
      const live = current.live?.servers.find(item => item.server_id === server.server_id);
      if (!live) throw new Error("서버 연결이 끊겼어요. 로그인 서버 연결을 다시 확인하고 있어요.");
      if (live.relation === "member") await openCentralMemberServer(live);
      else await openCentralOwnedServer(live);
    } catch (error) { setServerConnectionError(error instanceof Error ? error.message : "서버 연결이 끊겼어요."); }
    finally { serverOpening.current = false; setConnectingServerId(""); }
  }
  async function retryRoomConnection(room: RoomDockItem) {
    if (room.roomOrigin === "remote_server") {
      if (room.serverId && hasMemberAnchor(room.serverId)) {
        retryMemberHost(room.serverId);
        return;
      }
      await openRailServer({ server_id: room.serverId || "", alias: room.label, relation: "owner", host_os: null });
    } else {
      await refreshRoomDirectory(captureRoomDirectoryContinuity());
    }
  }
  return { openRailServer, retryRoomConnection, connectingServerId, serverConnectionError };
}
