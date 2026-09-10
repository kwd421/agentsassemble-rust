import { fetchJsonServerOperator, postJsonServerOperator } from "./http";
import { requestDesktopProviderDiscovery } from "../lib/desktopBridge";
import { providerCatalogIsValid } from "../lib/providerCatalogContract";
import type { ProviderCatalog } from "../types/generated/ProviderCatalog";

export async function refreshLocalProviderCatalog(providerId: string, force = true, signal?: AbortSignal): Promise<ProviderCatalog> {
  const generation = await requestDesktopProviderDiscovery(providerId, force);
  if (signal?.aborted) throw new DOMException("Provider discovery view closed.", "AbortError");
  const result = await fetchJsonServerOperator<unknown>(
    `/api/provider-catalog?provider_id=${encodeURIComponent(providerId)}&generation=${generation}`, undefined, signal
  );
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
  const statuses = cancel ? ["cancelled", "not_running"] : ["authenticated", "started"];
  if (Object.keys(value).length !== 2 || value.provider_id !== providerId ||
      typeof value.status !== "string" || !statuses.includes(value.status)) {
    throw new Error("Provider login response is invalid.");
  }
  return value.status;
}

export async function loginProvider(providerId: string): Promise<"authenticated" | "started"> {
  return await providerLoginOperation(providerId, false) as "authenticated" | "started";
}

export async function cancelProviderLogin(providerId: string): Promise<boolean> {
  return (await providerLoginOperation(providerId, true)) === "cancelled";
}

export async function providerUpdateOperation(providerId: string, expectedVersion?: string): Promise<import("../types/generated/ProviderUpdate").ProviderUpdate> {
  const result = await postJsonServerOperator<unknown>(
    expectedVersion === undefined ? "/api/providers/update/check" : "/api/providers/update/start",
    expectedVersion === undefined ? { provider_id: providerId } : { provider_id: providerId, expected_version: expectedVersion }
  );
  if (!result || typeof result !== "object" || Array.isArray(result)) throw new Error("버전 응답이 올바르지 않아요.");
  const value = result as Record<string, unknown>;
  if (Object.keys(value).length !== 7 || value.provider_id !== providerId ||
      typeof value.current_version !== "string" || typeof value.latest_version !== "string" ||
      typeof value.observed_at !== "string" || !Number.isFinite(Date.parse(value.observed_at)) ||
      typeof value.update_available !== "boolean" || typeof value.native_update !== "boolean" ||
      typeof value.completed !== "boolean" ||
      (expectedVersion !== undefined && !value.completed)) throw new Error("버전 응답이 올바르지 않아요.");
  return result as import("../types/generated/ProviderUpdate").ProviderUpdate;
}
