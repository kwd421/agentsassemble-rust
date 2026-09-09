import { fetchJsonServerOperator, postJsonServerOperator } from "./http";
import type { RuntimeRestartReceipt } from "../types/generated/RuntimeRestartReceipt";
import type { RuntimeRestartStatus } from "../types/generated/RuntimeRestartStatus";

export function readRuntimeRestart(operationId?: string, signal?: AbortSignal) {
  const query = operationId ? `?operation_id=${encodeURIComponent(operationId)}` : "";
  return fetchJsonServerOperator<RuntimeRestartStatus>(`/api/runtime/rolling-restart${query}`, undefined, signal);
}

export function requestRuntimeRestart(operationId: string, beforeDispatch: () => void, signal: AbortSignal) {
  return postJsonServerOperator<RuntimeRestartReceipt>("/api/runtime/rolling-restart", { operation_id: operationId }, beforeDispatch, signal);
}
