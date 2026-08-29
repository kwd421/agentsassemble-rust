import { fetchJson, fileToBase64, postJson } from "./http";
import type { PersonaAssetSummary } from "../types/generated/PersonaAssetSummary";

export type { PersonaAssetSummary };

export async function fetchPersonaAssets(): Promise<PersonaAssetSummary[]> {
  const payload = await fetchJson<{ items?: PersonaAssetSummary[] }>("/api/personas");
  return Array.isArray(payload.items) ? payload.items : [];
}

export async function importPersonaAsset(file: File): Promise<PersonaAssetSummary> {
  const dataBase64 = await fileToBase64(file);
  const payload = await postJson<{ persona: PersonaAssetSummary }>("/api/personas/import", {
    filename: file.name,
    data_base64: dataBase64,
  });
  return payload.persona;
}
