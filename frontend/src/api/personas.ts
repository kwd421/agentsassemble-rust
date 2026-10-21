import { fetchOwnerSession } from "../lib/ownerSessionTransport";
import { hasRemoteWorkspace, remoteWorkspaceFor, remoteWorkspacesSnapshot, fetchProductTransport } from "../lib/remote/remoteWorkspace";
import type { RemoteTransport } from "../lib/remote/remoteTransport";
import type { CentralOwnerWorkspace } from "../lib/central/ownerWorkspace";
import {
  deleteJsonServerOperator,
  fetchJsonServerOperator,
  serverOwnerSessionHeaders,
  fileToBase64,
  postJsonServerOperator,
  responseError,
} from "./http";
import {
  fetchDesktopOperatorRuntime,
  isDesktopWebview,
} from "../lib/desktopBridge";
import type { PersonaAssetSummary } from "../types/generated/PersonaAssetSummary";
import { strictPrivatePngBlob } from "./safeRaster";

export type { PersonaAssetSummary };

type PersonaHost = { owner: CentralOwnerWorkspace; deviceToken: string; transport: RemoteTransport | null };

// The persona library lives on the account's own server. Its workspace is
// resolved explicitly per call: the room's own server_id when given, else the
// single live owner workspace. With several hosts live an omitted server never
// falls back to "most recent" — a member host's transport is never used.
export function resolvePersonaHost(serverId?: string): PersonaHost | null {
  if (serverId) {
    const workspace = remoteWorkspaceFor(serverId);
    if (!workspace?.owner || !workspace.transport.active) {
      throw new Error("서버 소유자로 다시 연결해 주세요.");
    }
    return { owner: workspace.owner, deviceToken: workspace.deviceToken, transport: workspace.transport };
  }
  if (!hasRemoteWorkspace()) return null;
  const owners = remoteWorkspacesSnapshot().filter((entry) => entry.owner && entry.transport.active);
  if (owners.length === 1) return { owner: owners[0].owner!, deviceToken: owners[0].deviceToken, transport: owners[0].transport };
  // A member host never serves the persona library; several owners mean the
  // caller must name the room's host explicitly.
  if (owners.length === 0) throw new Error("서버 소유자로 다시 연결해 주세요.");
  throw new Error("서버를 확인하지 못했어요. 방의 서버에서 다시 시도해 주세요.");
}

export async function fetchPersonaAssets(serverId?: string): Promise<PersonaAssetSummary[]> {
  const host = resolvePersonaHost(serverId);
  const payload = host
    ? await (await remotePersona(host, "", { method: "GET" })).json() as { items?: PersonaAssetSummary[] }
    : await fetchJsonServerOperator<{ items?: PersonaAssetSummary[] }>("/api/personas");
  return Array.isArray(payload.items) ? payload.items : [];
}

export async function importPersonaAsset(file: File, serverId?: string): Promise<PersonaAssetSummary> {
  const host = resolvePersonaHost(serverId);
  const dataBase64 = await fileToBase64(file);
  if (host && !isCurrentPersonaHost(host)) throw new Error("서버가 바뀌었어요. 현재 서버에서 다시 시도해 주세요.");
  const body = { filename: file.name, data_base64: dataBase64 };
  const payload = host
    ? await (await remotePersona(host, "/import", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) })).json() as { persona: PersonaAssetSummary }
    : await postJsonServerOperator<{ persona: PersonaAssetSummary }>("/api/personas/import", body);
  return payload.persona;
}

export async function deletePersonaAsset(personaId: string, serverId?: string): Promise<void> {
  const host = resolvePersonaHost(serverId);
  const suffix = `/${encodeURIComponent(personaId)}`;
  if (host) await remotePersona(host, suffix, { method: "DELETE" });
  else await deleteJsonServerOperator(`/api/personas${suffix}`);
}

export async function fetchPersonaThumbnail(
  personaId: string,
  signal?: AbortSignal,
  serverId?: string
): Promise<Blob> {
  if (signal?.aborted) {
    throw signal.reason || new DOMException("Persona thumbnail read aborted.", "AbortError");
  }
  const path = `/api/personas/${encodeURIComponent(personaId)}/thumbnail`;
  const init: RequestInit = { cache: "no-store", signal };
  const host = resolvePersonaHost(serverId);
  const response = host ? await remotePersona(host, `/${encodeURIComponent(personaId)}/thumbnail`, init)
    : isDesktopWebview() ? await fetchDesktopOperatorRuntime(path, init)
    : await fetchProductTransport(path, init);
  if (!response.ok) throw await responseError(response);
  return strictPrivatePngBlob(
    response,
    "봇카드 썸네일 응답 계약이 올바르지 않습니다.",
    // The persona library lives on the resolved owner host: bind the blob
    // budget there, or read plainly for the local host. Never undefined with
    // a host live, so another host's budget is never charged.
    host ? host.transport : null
  );
}

function isCurrentPersonaHost(host: PersonaHost): boolean {
  const current = remoteWorkspaceFor(host.owner.serverId);
  return Boolean(current && current.owner && current.owner.sessionToken === host.owner.sessionToken && current.transport.active);
}

async function remotePersona(host: PersonaHost, suffix: string, init: RequestInit): Promise<Response> {
  return remoteOwnerRoute(host, `/api/central-owner/personas${suffix}`, init);
}

/** One server-owner route on the resolved host's secure channel. */
export async function remoteOwnerRoute(host: PersonaHost, path: string, init: RequestInit): Promise<Response> {
  if (!isCurrentPersonaHost(host)) throw new Error("서버 소유자로 다시 연결해 주세요.");
  const headers = new Headers(init.headers);
  for (const [key, value] of Object.entries(serverOwnerSessionHeaders(host.owner, host.deviceToken))) headers.set(key, value);
  const response = await fetchOwnerSession(host.owner.sessionToken, path, { ...init, headers, cache: "no-store", credentials: "omit", redirect: "error" });
  if (!response.ok) throw await responseError(response);
  return response;
}
