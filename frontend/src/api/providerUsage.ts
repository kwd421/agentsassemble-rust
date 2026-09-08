import { postJsonServerOperator } from "./http";
import type { ProviderUsage } from "../types/generated/ProviderUsage";
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

export async function readProviderUsage(providerId: string): Promise<ProviderUsage> {
  const value = await postJsonServerOperator<unknown>("/api/providers/usage", { provider_id: providerId });
  if (!record(value) || Object.keys(value).length !== 3 || value.provider_id !== providerId ||
      typeof value.observed_at !== "string" || !Number.isFinite(Date.parse(value.observed_at)) ||
      !record(value.quota) || Object.keys(value.quota).length !== 3 || value.quota.kind !== "balance" ||
      typeof value.quota.is_available !== "boolean" || !Array.isArray(value.quota.balances) ||
      value.quota.balances.length < 1 || value.quota.balances.length > 2 || !value.quota.balances.every(balance) ||
      new Set(value.quota.balances.map((item) => item.currency)).size !== value.quota.balances.length) {
    throw new Error("사용량 응답을 확인할 수 없어요. 다시 조회해 주세요.");
  }
  return value as unknown as ProviderUsage;
}
