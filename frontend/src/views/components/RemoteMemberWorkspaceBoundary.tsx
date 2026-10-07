import { useEffect, useState, type ReactNode } from "react";
import type { RemoteTransport } from "../../lib/remote/remoteTransport";
import { EndedWorkspace } from "./CentralOwnerWorkspaceBoundary";
export default function RemoteMemberWorkspaceBoundary({ transport, children }: { transport: RemoteTransport; children: ReactNode }) {
  const [ended, setEnded] = useState(!transport.active);
  useEffect(() => transport.onClose(() => setEnded(true)), [transport]);
  return <><div inert={ended || undefined} aria-hidden={ended || undefined} className="contents">{children}</div>{ended && <EndedWorkspace reason="disconnected" />}</>;
}
