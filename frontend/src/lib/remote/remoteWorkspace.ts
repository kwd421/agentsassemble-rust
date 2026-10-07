import type { RoomGuestSession } from "../roomGuestSession";
export type SecureMemberEntry = { server_id: string; registration_epoch: string; inviteToken?: string };
import type { CentralOwnerWorkspace } from "../central/ownerWorkspace";
import { RemoteTransport, connectionEnded } from "./remoteTransport";

type Workspace = { transport: RemoteTransport; owner: CentralOwnerWorkspace | null; member?: RoomGuestSession; deviceToken: string; clientId: string };
let workspace: Workspace | null = null;
let pendingMember: SecureMemberEntry | null = null;
export const pendingMemberSnapshot = () => pendingMember;
export function selectRemoteMember(entry: SecureMemberEntry | null) { pendingMember = entry; for (const listener of listeners) listener(); }
const listeners = new Set<() => void>();
// Entries remain until page disposal, including after a channel ends: an old
// bearer must never fall through to a plaintext request.
const sessions = new Map<string, RemoteTransport | null>();
export const remoteWorkspaceSnapshot = () => workspace;
export function subscribeRemoteWorkspace(listener: () => void) {
  listeners.add(listener); return () => { listeners.delete(listener); };
}
export function bindRemoteSession(token: string, transport: RemoteTransport) {
  sessions.set(token, transport);
  transport.onClose(() => { if (sessions.get(token) === transport) sessions.set(token, null); });
}
export function remoteSessionTransport(token?: string) { return token ? sessions.get(token) ?? undefined : undefined; }
export function installRemoteWorkspace(next: Workspace) {
  workspace?.transport.close();
  workspace = next; pendingMember = null;
  bindRemoteSession(next.owner?.sessionToken || next.member!.sessionToken, next.transport);
  for (const listener of listeners) listener();
}
export function closeRemoteWorkspace() {
  workspace?.transport.close(); workspace = null;
  for (const listener of listeners) listener();
}
export function fetchSessionTransport(token: string | undefined, path: string, init: RequestInit = {}): Promise<Response> {
  const transport = remoteSessionTransport(token);
  if (transport) return transport.fetch(path, init);
  if (workspace || token && sessions.has(token)) return Promise.reject(connectionEnded());
  return fetch(path, init);
}

export function fetchProductTransport(path: string, init: RequestInit = {}): Promise<Response> {
  const authorization = new Headers(init.headers).get("Authorization");
  return fetchSessionTransport(authorization?.startsWith("Bearer ") ? authorization.slice(7) : undefined, path, init);
}
