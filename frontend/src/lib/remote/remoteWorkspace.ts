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
export function selectRemoteMember(entry: SecureMemberEntry | null) { pendingMember = entry; notifyWorkspaceListeners(); }
const listeners = new Set<() => void>();
let workspaceVersion = 0;
function notifyWorkspaceListeners() {
  workspaceVersion += 1;
  for (const listener of listeners) listener();
}
export const remoteWorkspaceVersion = () => workspaceVersion;
// Entries remain until page disposal, including after a channel ends: an old
// bearer must never fall through to a plaintext request.
const sessions = new Map<string, { transport: RemoteTransport | null; origin: string; serverId: string }>();
// No "most recent workspace" selector exists on purpose: with several hosts
// live at once, picking one implicitly would send one host's bearer, upload or
// read to another host. Every per-host call resolves its workspace explicitly
// from the room's own server_id/origin (remoteWorkspaceFor) or from its own
// bearer (remoteSessionTransport); App-wide UI uses hasRemoteWorkspace().
export const remoteWorkspacesSnapshot = (): Workspace[] => [...workspaces.values()];
export const remoteWorkspaceFor = (serverId: string): Workspace | null => workspaces.get(serverId) ?? null;
export function remoteTransportForOrigin(origin: string): RemoteTransport | null {
  for (const entry of workspaces.values()) {
    if (entry.transport.hello.origin === origin && entry.transport.active) return entry.transport;
  }
  // Background member rooms bind their sessions without installing a
  // workspace (the foreground session alone owns the workspace slot), so a
  // member host that is attached but not open is found here too.
  for (const binding of sessions.values()) {
    if (binding.origin === origin && binding.transport?.active) return binding.transport;
  }
  return null;
}
// Whether an origin belongs to a host the client has bound (installed
// workspace or bound session, live or not). Image render paths use this to
// fail closed: a host-origin URL without a live transport renders nothing,
// never a direct tunnel URL (desktop CSP blocks it and it carries no auth).
export function isKnownHostOrigin(origin: string): boolean {
  for (const entry of workspaces.values()) {
    if (entry.transport.hello.origin === origin) return true;
  }
  for (const binding of sessions.values()) {
    if (binding.origin === origin) return true;
  }
  return false;
}
export const hasRemoteWorkspace = (): boolean => workspaces.size > 0;
/**
 * Default origin for host-agnostic reads. With exactly one host live it is
 * that host (the old single-host behavior, with nothing to leak to); with
 * zero or several live it is the local origin, so per-host lookups bound to
 * a real host origin simply miss instead of hitting another host.
 */
export function defaultWorkspaceOrigin(): string {
  const live = [...workspaces.values()];
  if (live.length === 1) return live[0].transport.hello.origin;
  return typeof window !== "undefined" ? window.location.origin : "";
}
/**
 * Fail-closed host origin for a bearer. A session bound to its own host
 * resolves there; an unknown session fails closed while any host is live.
 * With no host live there is no other host to leak to, so the legacy local
 * origin applies (single-host startup/admission paths).
 */
/**
 * Display-only host origin for a bearer: the bound host, else the legacy
 * local origin when no host is live, else null (never another host). Unlike
 * requireSessionHostOrigin it never throws, so render paths stay total.
 */
export function sessionHostDisplayOrigin(token: string): string | null {
  return sessionHostOrigin(token) ?? (hasRemoteWorkspace() ? null : typeof window !== "undefined" ? window.location.origin : null);
}
export function requireSessionHostOrigin(token: string): string {
  const origin = sessionHostOrigin(token);
  if (origin) return origin;
  if (!hasRemoteWorkspace()) return typeof window !== "undefined" ? window.location.origin : "";
  throw new Error("서버 연결이 끝났어요. 서버 목록에서 다시 연결해 주세요.");
}
export function requireRemoteWorkspaceFor(serverId: string): Workspace {
  const workspace = serverId ? workspaces.get(serverId) ?? null : null;
  if (!workspace || !workspace.transport.active) throw new Error("서버 연결이 끝났어요. 서버 목록에서 다시 연결해 주세요.");
  return workspace;
}
export function subscribeRemoteWorkspace(listener: () => void) {
  listeners.add(listener); return () => { listeners.delete(listener); };
}
export function bindRemoteSession(token: string, transport: RemoteTransport, serverId?: string) {
  const binding = { transport: transport as RemoteTransport | null, origin: transport.hello.origin, serverId: serverId ?? "" };
  sessions.set(token, binding);
  transport.onClose(() => { if (sessions.get(token) === binding) binding.transport = null; });
}
export function remoteSessionTransport(token?: string) { return token ? sessions.get(token)?.transport ?? undefined : undefined; }
/** Drops every bound bearer of one host after confirmed membership loss. */
export function unbindRemoteSessionsForServer(serverId: string): void {
  if (!serverId) return;
  for (const [token, binding] of sessions) {
    if (binding.serverId === serverId) sessions.delete(token);
  }
}
/** Fail-closed host origin for a bearer: null when the session is unknown or closed. */
export function sessionHostOrigin(token: string): string | null { return sessions.get(token)?.origin ?? null; }
/** Fail-closed server id for a bearer: null when the session is unknown. */
export function sessionHostServerId(token: string): string | null {
  const serverId = sessions.get(token)?.serverId ?? "";
  return serverId ? serverId : null;
}
export function installRemoteWorkspace(next: WorkspaceInstall) {
  const serverId = next.owner?.serverId || next.member?.serverSurface.server_id || "";
  if (!serverId) throw new Error("서버를 확인하지 못했어요. 서버 목록에서 다시 연결해 주세요.");
  const advertised = next.transport.hello.server_id;
  if (advertised && advertised !== serverId) throw new Error("선택한 서버와 연결이 일치하지 않습니다.");
  const existing = workspaces.get(serverId);
  if (existing && existing.transport === next.transport) {
    // Same live channel: never close it; only bind the new room session and
    // keep the workspace on the live transport.
    workspaces.set(serverId, { ...next, serverId });
    pendingMember = null;
    bindRemoteSession(next.owner?.sessionToken || next.member!.sessionToken, next.transport, serverId);
    notifyWorkspaceListeners();
    return;
  }
  // Replacing one host closes only that host's previous transport.
  existing?.transport.close();
  workspaces.delete(serverId);
  workspaces.set(serverId, { ...next, serverId });
  pendingMember = null;
  bindRemoteSession(next.owner?.sessionToken || next.member!.sessionToken, next.transport, serverId);
  notifyWorkspaceListeners();
}
export function closeRemoteWorkspace(serverId?: string) {
  if (serverId === undefined) {
    for (const entry of workspaces.values()) entry.transport.close();
    workspaces.clear();
  } else {
    workspaces.get(serverId)?.transport.close();
    workspaces.delete(serverId);
  }
  notifyWorkspaceListeners();
}
const plainUploads = new Map<string, { transfer: boolean; limit: number }>();
export function messageAttachmentCapabilities(token: string, path: string, local = false) {
  const transport = remoteSessionTransport(token);
  if (transport) return transport.messageAttachmentCapabilities();
  // A local (desktop-ticket) authority never resolves to a remote transport;
  // it reads the plain probe cache even while remote hosts are live. A remote
  // bearer with no live transport fails closed instead of downgrading.
  if (!local && (workspaces.size > 0 || sessions.has(token))) throw connectionEnded();
  return plainUploads.get(requestOrigin(path)) ?? { transfer: false, limit: LEGACY_MESSAGE_ATTACHMENT_LIMIT };
}
const plainImagePreviews = new Map<string, boolean>();
function requestOrigin(path: string) { return new URL(path, window.location.href).origin; }
export function supportsMessageAttachmentPreview(token: string, path: string, local = false): boolean {
  const transport = remoteSessionTransport(token);
  if (transport) return transport.supportsImagePreview();
  if (!local && (workspaces.size > 0 || sessions.has(token))) return false;
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
