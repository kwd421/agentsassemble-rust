import { CentralOwnerWorkspaceContext } from "../lib/central/ownerWorkspaceContext";
import { useCallback, useContext, useEffect, useMemo, useRef } from "react";
import { createCentralOwnerRoom, enterCentralOwnerRoom, fetchCentralOwnerRooms, openCentralOwnerDirectoryStream } from "../lib/central/ownerWorkspace";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { OperatorPairingRedeemResponse } from "../lib/roomAdmissionContract";
import type { createRoom } from "../api";

export function useCentralOwnerWorkspace(deviceToken: string) {
  const workspace = useContext(CentralOwnerWorkspaceContext);
  const connect = workspace?.session;
  const active = useRef(false);
  const inFlight = useRef(false);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  const remoteDirectory = useMemo(() => connect ? {
    serverId: connect.serverId,
    onStatus: workspace.onStatus,
    openStream: (signal: AbortSignal) => openCentralOwnerDirectoryStream(connect, deviceToken, signal),
    fetchRooms: (beforeDispatch: () => void, signal?: AbortSignal) => {
      beforeDispatch();
      return fetchCentralOwnerRooms(connect, deviceToken, signal);
    },
  } : undefined, [connect, deviceToken, workspace]);
  const create = useMemo<typeof createRoom | undefined>(() => connect
    ? (requestId, roomId, label = "", beforeDispatch) => {
        beforeDispatch?.();
        return createCentralOwnerRoom(connect, deviceToken, requestId, roomId, label);
      }
    : undefined, [connect, deviceToken]);
  const enter = useCallback(async (room: RoomDockItem, accept: (payload: OperatorPairingRedeemResponse) => Promise<boolean>) => {
    if (!connect || inFlight.current) return false;
    if (room.serverId !== connect.serverId || !room.roomUid) throw new Error("확인된 서버의 방만 열 수 있어요.");
    inFlight.current = true;
    try {
      const payload = await enterCentralOwnerRoom(connect, deviceToken, room.meetingId, room.roomUid);
      if (!active.current) return false;
      return await accept(payload);
    } finally { inFlight.current = false; }
  }, [connect, deviceToken]);
  return { connect, remoteDirectory, create, enter };
}
