import { fetchOwnerSession } from "../lib/ownerSessionTransport";
import { remoteWorkspaceSnapshot, fetchProductTransport } from "../lib/remote/remoteWorkspace";
import {
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

export async function fetchPersonaAssets(): Promise<PersonaAssetSummary[]> {
  const remote = remoteWorkspaceSnapshot();
  const payload = remote
    ? await (await remotePersona(remote, "", { method: "GET" })).json() as { items?: PersonaAssetSummary[] }
    : await fetchJsonServerOperator<{ items?: PersonaAssetSummary[] }>("/api/personas");
  return Array.isArray(payload.items) ? payload.items : [];
}

export async function importPersonaAsset(file: File): Promise<PersonaAssetSummary> {
  const remote = remoteWorkspaceSnapshot();
  const dataBase64 = await fileToBase64(file);
  if (remoteWorkspaceSnapshot() !== remote) throw new Error("서버가 바뀌었어요. 현재 서버에서 다시 시도해 주세요.");
  const body = { filename: file.name, data_base64: dataBase64 };
  const payload = remote
    ? await (await remotePersona(remote, "/import", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) })).json() as { persona: PersonaAssetSummary }
    : await postJsonServerOperator<{ persona: PersonaAssetSummary }>("/api/personas/import", body);
  return payload.persona;
}

export async function fetchPersonaThumbnail(
  personaId: string,
  signal?: AbortSignal
): Promise<Blob> {
  if (signal?.aborted) {
    throw signal.reason || new DOMException("Persona thumbnail read aborted.", "AbortError");
  }
  const path = `/api/personas/${encodeURIComponent(personaId)}/thumbnail`;
  const init: RequestInit = { cache: "no-store", signal };
  const remote = remoteWorkspaceSnapshot();
  const response = remote ? await remotePersona(remote, `/${encodeURIComponent(personaId)}/thumbnail`, init)
    : isDesktopWebview() ? await fetchDesktopOperatorRuntime(path, init)
    : await fetchProductTransport(path, init);
  if (!response.ok) throw await responseError(response);
  return strictPrivatePngBlob(
    response,
    "봇카드 썸네일 응답 계약이 올바르지 않습니다."
  );
}

async function remotePersona(remote: NonNullable<ReturnType<typeof remoteWorkspaceSnapshot>>, suffix: string, init: RequestInit): Promise<Response> {
  if (!remote.owner || remoteWorkspaceSnapshot() !== remote || !remote.transport.active) throw new Error("서버 소유자로 다시 연결해 주세요.");
  const headers = new Headers(init.headers);
  for (const [key, value] of Object.entries(serverOwnerSessionHeaders(remote.owner, remote.deviceToken))) headers.set(key, value);
  const response = await fetchOwnerSession(remote.owner.sessionToken, `/api/central-owner/personas${suffix}`, { ...init, headers, cache: "no-store", credentials: "omit", redirect: "error" });
  if (!response.ok) throw await responseError(response);
  return response;
}
