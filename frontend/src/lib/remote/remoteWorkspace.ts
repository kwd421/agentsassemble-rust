import { LEGACY_MESSAGE_ATTACHMENT_LIMIT, parseMessageAttachmentLimit } from "../messageAttachmentCapabilities";
import type { RoomGuestSession } from "../roomGuestSession";
export type SecureMemberEntry = { server_id: string; registration_epoch: string; inviteToken?: string };
import type { CentralOwnerWorkspace } from "../central/ownerWorkspace";
import { RemoteTransport, connectionEnded } from "./remoteTransport";

type WorkspaceInstall = { transport: RemoteTransport; owner: CentralOwnerWorkspace | null; member?: RoomGuestSession; deviceToken: string; clientId: string };
type Workspace = WorkspaceInstall & { serverId: string };
// One live entry per host, keyed by server_id in install order. Installing
// host B never closes host A: each entry owns its transport and session,
// and routing below always resolves the bearer to its own transport.
const workspaces = new Map<string, Workspace>();
let pendingMember: SecureMemberEntry | null = null;
export const pendingMemberSnapshot = () => pendingMember;
export function selectRemoteMember(entry: SecureMemberEntry | null) { pendingMember = entry; for (const listener of listeners) listener(); }
const listeners = new Set<() => void>();
// Entries remain until page disposal, including after a channel ends: an old
// bearer must never fall through to a plaintext request.
const sessions = new Map<string, { transport: RemoteTransport | null; origin: string }>();
function lastWorkspace(): Workspace | null {
  let current: Workspace | null = null;
  for (const entry of workspaces.values()) current = entry;
  return current;
}
// Transitional single-context selector: the most recently installed live
// entry. Per-host consumers resolve through remoteWorkspaceFor or the bearer
// binding instead.
export const remoteWorkspaceSnapshot = () => lastWorkspace();
export const remoteWorkspacesSnapshot = (): Workspace[] => [...workspaces.values()];
export const remoteWorkspaceFor = (serverId: string): Workspace | null => workspaces.get(serverId) ?? null;
export function subscribeRemoteWorkspace(listener: () => void) {
  listeners.add(listener); return () => { listeners.delete(listener); };
}
export function bindRemoteSession(token: string, transport: RemoteTransport) {
  const binding = { transport: transport as RemoteTransport | null, origin: transport.hello.origin };
  sessions.set(token, binding);
  transport.onClose(() => { if (sessions.get(token) === binding) binding.transport = null; });
}
export function remoteSessionTransport(token?: string) { return token ? sessions.get(token)?.transport ?? undefined : undefined; }
export function productWorkspaceOrigin() { return lastWorkspace()?.transport.hello.origin ?? (typeof window !== "undefined" ? window.location.origin : ""); }
export function sessionHostOrigin(token: string) { return sessions.get(token)?.origin ?? (typeof window !== "undefined" ? window.location.origin : ""); }
export function installRemoteWorkspace(next: WorkspaceInstall) {
  const serverId = next.owner?.serverId || next.member?.serverSurface.server_id || "";
  if (!serverId) throw new Error("서버를 확인하지 못했어요. 서버 목록에서 다시 연결해 주세요.");
  const advertised = next.transport.hello.server_id;
  if (advertised && advertised !== serverId) throw new Error("선택한 서버와 연결이 일치하지 않습니다.");
  // Replacing one host closes only that host's previous transport.
  workspaces.get(serverId)?.transport.close();
  workspaces.delete(serverId);
  workspaces.set(serverId, { ...next, serverId });
  pendingMember = null;
  bindRemoteSession(next.owner?.sessionToken || next.member!.sessionToken, next.transport);
  for (const listener of listeners) listener();
}
export function closeRemoteWorkspace(serverId?: string) {
  if (serverId === undefined) {
    for (const entry of workspaces.values()) entry.transport.close();
    workspaces.clear();
  } else {
    workspaces.get(serverId)?.transport.close();
    workspaces.delete(serverId);
  }
  for (const listener of listeners) listener();
}
const plainUploads = new Map<string, { transfer: boolean; limit: number }>();
export function messageAttachmentCapabilities(token: string, path: string) {
  const transport = remoteSessionTransport(token);
  if (transport) return transport.messageAttachmentCapabilities();
  if (workspaces.size > 0 || sessions.has(token)) throw connectionEnded();
  return plainUploads.get(requestOrigin(path)) ?? { transfer: false, limit: LEGACY_MESSAGE_ATTACHMENT_LIMIT };
}
const plainImagePreviews = new Map<string, boolean>();
function requestOrigin(path: string) { return new URL(path, window.location.href).origin; }
export function supportsMessageAttachmentPreview(token: string, path: string): boolean {
  const transport = remoteSessionTransport(token);
  if (transport) return transport.supportsImagePreview();
  if (workspaces.size > 0 || sessions.has(token)) return false;
  return plainImagePreviews.get(requestOrigin(path)) === true;
}

export function fetchPlainProductTransport(path: string, init: RequestInit = {}): Promise<Response> {
  // A host must not be able to send this request's credentials or body elsewhere.
  return fetch(path, { ...init, redirect: "error", referrerPolicy: "no-referrer" }).then(response => {
    // GET room-settings is the successful admission/probe response. Unrelated
    // responses (including uploads, errors and redirects) cannot rewrite it.
    if ((init.method ?? "GET").toUpperCase() !== "GET" ||
        new URL(path, window.location.href).pathname !== "/api/room-settings" ||
        !response.ok || response.redirected) return response;
    plainUploads.set(requestOrigin(path), {
      transfer: response.headers.get("x-agentsassemble-upload-transfer") === "1",
      limit: parseMessageAttachmentLimit(response.headers.get("x-agentsassemble-message-attachment-limit")),
    });
    plainImagePreviews.set(requestOrigin(path), response.headers.get("x-agentsassemble-image-preview") === "1");
    return response;
  });
}

export function fetchSessionTransport(token: string | undefined, path: string, init: RequestInit = {}): Promise<Response> {
  const transport = remoteSessionTransport(token);
  if (transport) return transport.fetch(path, init);
  if (workspaces.size > 0 || token && sessions.has(token)) return Promise.reject(connectionEnded());
  return fetchPlainProductTransport(path, init);
}

export function fetchProductTransport(path: string, init: RequestInit = {}): Promise<Response> {
  const authorization = new Headers(init.headers).get("Authorization");
  return fetchSessionTransport(authorization?.startsWith("Bearer ") ? authorization.slice(7) : undefined, path, init);
}
