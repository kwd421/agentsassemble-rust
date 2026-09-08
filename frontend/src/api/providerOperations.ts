import { postEmptyServerOperator, postJsonServerOperator } from "./http";
import { providerCatalogIsValid } from "../lib/providerCatalogContract";
import type { ProviderCatalog } from "../types/generated/ProviderCatalog";

export async function refreshProviderCatalog(): Promise<ProviderCatalog> {
  const result = await postEmptyServerOperator<unknown>("/api/provider-catalog/refresh");
  if (!providerCatalogIsValid(result)) throw new Error("Provider catalog response is invalid.");
  if (result.status !== "ready") throw new Error("카탈로그를 갱신하지 못했어요. 다시 시도해 주세요.");
  return result;
}

async function providerLoginOperation(providerId: string, cancel: boolean): Promise<string> {
  const result = await postJsonServerOperator<unknown>(
    cancel ? "/api/providers/login/cancel" : "/api/providers/login", { provider_id: providerId }
  );
  if (!result || typeof result !== "object" || Array.isArray(result)) throw new Error("Provider login response is invalid.");
  const value = result as Record<string, unknown>;
  const statuses = cancel ? ["cancelled", "not_running"] : ["authenticated"];
  if (Object.keys(value).length !== 2 || value.provider_id !== providerId ||
      typeof value.status !== "string" || !statuses.includes(value.status)) {
    throw new Error("Provider login response is invalid.");
  }
  return value.status;
}

export async function loginProvider(providerId: string): Promise<void> {
  await providerLoginOperation(providerId, false);
}

export async function cancelProviderLogin(providerId: string): Promise<boolean> {
  return (await providerLoginOperation(providerId, true)) === "cancelled";
}
