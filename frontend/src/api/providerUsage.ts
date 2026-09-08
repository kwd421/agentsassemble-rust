import { postJsonServerOperator } from "./http";
import type { ProviderUsage } from "../types/generated/ProviderUsage";
import type { ProviderQuota } from "../types/generated/ProviderQuota";
import type { ProviderRateWindow } from "../types/generated/ProviderRateWindow";
import type { ProviderBalance } from "../types/generated/ProviderBalance";

function record(value: unknown): value is Record<string, unknown> {
  return !!value && typeof value === "object" && !Array.isArray(value);
}
function balance(value: unknown): value is ProviderBalance {
  return record(value) && Object.keys(value).length === 4 &&
    typeof value.currency === "string" && ["USD", "CNY"].includes(value.currency) &&
    [value.total_balance, value.granted_balance, value.topped_up_balance].every(
      (amount) => typeof amount === "string" && amount.length <= 128 && /^-?\d+(\.\d+)?$/.test(amount)
    );
}

function rateWindow(value: unknown): value is ProviderRateWindow {
  return record(value) && Object.keys(value).length === 5 &&
    [value.id, value.label].every((text) => typeof text === "string" && text.length > 0 && text.length <= 128) &&
    (value.used_percent === null || typeof value.used_percent === "number" && Number.isFinite(value.used_percent) && value.used_percent >= 0) &&
    (value.resets_at === null || typeof value.resets_at === "string" && Number.isFinite(Date.parse(value.resets_at))) &&
    (value.window_minutes === null || typeof value.window_minutes === "number" && Number.isSafeInteger(value.window_minutes) && value.window_minutes > 0);
}
function quota(value: unknown): value is ProviderQuota {
  if (!record(value) || Object.keys(value).length !== 3) return false;
  if (value.kind === "balance") return typeof value.is_available === "boolean" && Array.isArray(value.balances) &&
    value.balances.length >= 1 && value.balances.length <= 2 && value.balances.every(balance) &&
    new Set(value.balances.map((item) => item.currency)).size === value.balances.length;
  return value.kind === "rate_limits" && typeof value.available === "boolean" && Array.isArray(value.windows) &&
    value.windows.length <= 64 && value.windows.every(rateWindow) &&
    new Set(value.windows.map((item) => item.id)).size === value.windows.length &&
    (value.available || value.windows.length === 0);
}

export async function readProviderUsage(providerId: string): Promise<ProviderUsage> {
  const value = await postJsonServerOperator<unknown>("/api/providers/usage", { provider_id: providerId });
  if (!record(value) || Object.keys(value).length !== 3 || value.provider_id !== providerId ||
      typeof value.observed_at !== "string" || !Number.isFinite(Date.parse(value.observed_at)) ||
      !quota(value.quota)) {
    throw new Error("사용량 응답을 확인할 수 없어요. 다시 조회해 주세요.");
  }
  return value as unknown as ProviderUsage;
}
