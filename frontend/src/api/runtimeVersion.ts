import type { RuntimeVersion } from "../types/generated/RuntimeVersion";

export async function readRuntimeVersion(signal: AbortSignal): Promise<RuntimeVersion> {
  const response = await fetch("/api/runtime/version", {
    cache: "no-store", credentials: "same-origin", signal,
  });
  if (!response.ok) throw new Error("화면 버전을 확인하지 못했어요.");
  const value: unknown = await response.json();
  if (!value || typeof value !== "object" || Array.isArray(value)) {
    throw new Error("화면 버전 응답을 확인하지 못했어요.");
  }
  const record = value as Record<string, unknown>;
  if ((record.frontend_build_id !== null &&
    (typeof record.frontend_build_id !== "string" || !/^[a-f0-9]{64}$/.test(record.frontend_build_id))) ||
    typeof record.protocol_version !== "number" || !Number.isSafeInteger(record.protocol_version) ||
    record.protocol_version < 0) {
    throw new Error("화면 버전 응답을 확인하지 못했어요.");
  }
  return { frontend_build_id: record.frontend_build_id as string | null, protocol_version: record.protocol_version };
}
