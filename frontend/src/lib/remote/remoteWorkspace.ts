import { LEGACY_MESSAGE_ATTACHMENT_LIMIT, parseMessageAttachmentLimit } from "../messageAttachmentCapabilities";
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
const sessions = new Map<string, { transport: RemoteTransport | null; origin: string }>();
export const remoteWorkspaceSnapshot = () => workspace;
export function subscribeRemoteWorkspace(listener: () => void) {
  listeners.add(listener); return () => { listeners.delete(listener); };
}
export function bindRemoteSession(token: string, transport: RemoteTransport) {
  const binding = { transport: transport as RemoteTransport | null, origin: transport.hello.origin };
  sessions.set(token, binding);
  transport.onClose(() => { if (sessions.get(token) === binding) binding.transport = null; });
}
export function remoteSessionTransport(token?: string) { return token ? sessions.get(token)?.transport ?? undefined : undefined; }
export function productWorkspaceOrigin() { return workspace?.transport.hello.origin ?? (typeof window !== "undefined" ? window.location.origin : ""); }
export function sessionHostOrigin(token: string) { return sessions.get(token)?.origin ?? (typeof window !== "undefined" ? window.location.origin : ""); }
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
const plainUploads = new Map<string, { transfer: boolean; limit: number }>();
export function messageAttachmentCapabilities(token: string, path: string) {
  const transport = remoteSessionTransport(token);
  if (transport) return transport.messageAttachmentCapabilities();
  if (workspace || sessions.has(token)) throw connectionEnded();
  return plainUploads.get(requestOrigin(path)) ?? { transfer: false, limit: LEGACY_MESSAGE_ATTACHMENT_LIMIT };
}
const plainImagePreviews = new Map<string, boolean>();
function requestOrigin(path: string) { return new URL(path, window.location.href).origin; }
export function supportsMessageAttachmentPreview(token: string, path: string): boolean {
  const transport = remoteSessionTransport(token);
  if (transport) return transport.supportsImagePreview();
  if (workspace || sessions.has(token)) return false;
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
  if (workspace || token && sessions.has(token)) return Promise.reject(connectionEnded());
  return fetchPlainProductTransport(path, init);
}

export function fetchProductTransport(path: string, init: RequestInit = {}): Promise<Response> {
  const authorization = new Headers(init.headers).get("Authorization");
  return fetchSessionTransport(authorization?.startsWith("Bearer ") ? authorization.slice(7) : undefined, path, init);
}
