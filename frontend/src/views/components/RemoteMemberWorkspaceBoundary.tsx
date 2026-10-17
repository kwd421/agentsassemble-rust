import { useEffect, useState, type ReactNode } from "react";
import type { RemoteTransport } from "../../lib/remote/remoteTransport";
import { EndedWorkspace } from "./CentralOwnerWorkspaceBoundary";
export default function RemoteMemberWorkspaceBoundary({ transport, serverId, children }: { transport: RemoteTransport; serverId?: string; children: ReactNode }) {
  const [ended, setEnded] = useState(!transport.active);
  useEffect(() => transport.onClose(() => setEnded(true)), [transport]);
  return <><div inert={ended || undefined} aria-hidden={ended || undefined} className="contents">{children}</div>{ended && <EndedWorkspace reason="disconnected" serverId={serverId} />}</>;
}
/** End-state overlay without wrapping children: lets member-only installs keep the app content mounted. */
export function MemberEndedOverlay({ transport, serverId }: { transport: RemoteTransport; serverId?: string }) {
  const [ended, setEnded] = useState(!transport.active);
  useEffect(() => transport.onClose(() => setEnded(true)), [transport]);
  if (!ended) return null;
  return <EndedWorkspace reason="disconnected" serverId={serverId} />;
}
