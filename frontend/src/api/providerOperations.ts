import { postEmptyServerOperator } from "./http";
import { providerCatalogIsValid } from "../lib/providerCatalogContract";
import type { ProviderCatalog } from "../types/generated/ProviderCatalog";

export async function refreshProviderCatalog(): Promise<ProviderCatalog> {
  const result = await postEmptyServerOperator<unknown>("/api/provider-catalog/refresh");
  if (!providerCatalogIsValid(result)) throw new Error("Provider catalog response is invalid.");
  if (result.status !== "ready") throw new Error("카탈로그를 갱신하지 못했어요. 다시 시도해 주세요.");
  return result;
}
