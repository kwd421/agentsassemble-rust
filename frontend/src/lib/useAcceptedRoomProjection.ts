import { useCallback, useRef, useState } from "react";

import type { RoomSocketHandle } from "../roomSocketClient";

type AcceptedProjection = {
  scope: string;
  displayResourceBase: string;
};

const EMPTY_PROJECTION: AcceptedProjection = {
  scope: "",
  displayResourceBase: "",
};

/** Owns the socket generation and display origin accepted for one room projection. */
export function useAcceptedRoomProjection(scope: string) {
  const [accepted, setAccepted] = useState<AcceptedProjection>(EMPTY_PROJECTION);
  const acceptedRef = useRef<AcceptedProjection>(EMPTY_PROJECTION);
  const socketRef = useRef<RoomSocketHandle | null>(null);

  const clear = useCallback(() => {
    acceptedRef.current = EMPTY_PROJECTION;
    socketRef.current = null;
    setAccepted(EMPTY_PROJECTION);
  }, []);

  const accept = useCallback((socket: RoomSocketHandle, displayResourceBase: string) => {
    const next = { scope, displayResourceBase };
    acceptedRef.current = next;
    socketRef.current = socket;
    setAccepted(next);
  }, [scope]);

  const scopeIsAccepted = useCallback(
    () => acceptedRef.current.scope === scope,
    [scope]
  );

  const socketIsAccepted = useCallback(
    (socket: RoomSocketHandle) =>
      acceptedRef.current.scope === scope && socketRef.current === socket,
    [scope]
  );

  return { accepted, accept, clear, scopeIsAccepted, socketIsAccepted };
}
