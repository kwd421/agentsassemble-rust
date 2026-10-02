import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createCentralOwnerRoom, enterCentralOwnerRoom, fetchCentralOwnerRooms, loadCentralOwnerWorkspace } from "../lib/centralOwnerConnect";
import type { RoomDockItem } from "../lib/roomDockModel";
import type { OperatorPairingRedeemResponse } from "../lib/roomAdmissionContract";
import type { createRoom } from "../api";

export function useCentralOwnerWorkspace(deviceToken: string) {
  const [connect] = useState(loadCentralOwnerWorkspace);
  const active = useRef(false);
  const inFlight = useRef(false);
  useEffect(() => { active.current = true; return () => { active.current = false; }; }, []);
  const remoteDirectory = useMemo(() => connect ? {
    serverId: connect.serverId,
    fetchRooms: (beforeDispatch: () => void) => {
      beforeDispatch();
      return fetchCentralOwnerRooms(connect, deviceToken);
    },
  } : undefined, [connect, deviceToken]);
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
