import {
  fetchJsonServerOperator,
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
  const payload = await fetchJsonServerOperator<{ items?: PersonaAssetSummary[] }>(
    "/api/personas"
  );
  return Array.isArray(payload.items) ? payload.items : [];
}

export async function importPersonaAsset(file: File): Promise<PersonaAssetSummary> {
  const dataBase64 = await fileToBase64(file);
  const payload = await postJsonServerOperator<{ persona: PersonaAssetSummary }>(
    "/api/personas/import",
    {
      filename: file.name,
      data_base64: dataBase64,
    }
  );
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
  const response = isDesktopWebview()
    ? await fetchDesktopOperatorRuntime(path, init)
    : await fetch(path, init);
  if (!response.ok) throw await responseError(response);
  return strictPrivatePngBlob(
    response,
    "봇카드 썸네일 응답 계약이 올바르지 않습니다."
  );
}
